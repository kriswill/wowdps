//! Hyprland workspace tracking (`follow_game`): hide the overlay whenever
//! the game's workspace is not on any screen.
//!
//! A layer-shell surface belongs to an output, not a workspace, so the
//! compositor draws it over every workspace. To restrict the overlay to the
//! workspace World of Warcraft occupies, a thread watches Hyprland's IPC
//! event stream and, on every event that could change what is on screen,
//! re-asks "is the game's workspace the active one on some monitor?". Only
//! transitions are pushed over the channel; the overlay collapses to a
//! click-through pixel on `false` and restores itself on `true`.
//!
//! Two games' windows may be open at once, alike in class and title: the
//! game's is then the one whose process's command line names the live
//! edition by its product folder (`wowdps_model::product_of`), which the
//! overlay's session hears from the daemon and hands the tracker
//! ([`Tracker::set_live`]). With no live edition, or none of the windows
//! its, the best match listed first is the game's, as it always was. When
//! the live edition changes, the tracker asks again at once and tells the
//! overlay the monitor that edition's window is on ([`Seen::Monitor`]).
//!
//! Everything is stdlib. Hyprland exposes two unix sockets under
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/`: `.socket.sock`
//! answers one request per connection, `.socket2.sock` streams one event per
//! line. Replies are requested in hyprctl's plain-text format and parsed by
//! hand, keeping serde_json out of the tree. Every failure mode — not under
//! Hyprland, socket gone, unparsable reply, no game window — resolves to
//! "visible": tracking trouble must never hide the meter mid-pull.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use wowdps_model::{Edition, product_of};

/// What the tracker tells the overlay, in the order it learns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Whether the game's workspace is on some screen: sent on a change.
    Visible(bool),
    /// The monitor (`DP-1`) the live edition's window is on, told once
    /// after the live edition changed, as soon as a window whose process
    /// names it is found: where the overlay belongs now.
    Monitor(String),
}

/// The overlay's ends of the tracking threads: what they see, and the live
/// edition they look for. Dropping it ends both: the edition thread at
/// once, the event thread at its next transition.
pub struct Tracker {
    seen: Receiver<Seen>,
    live: Sender<Option<Edition>>,
}

impl Tracker {
    /// Look on `dir`'s Hyprland for the windows `game_match` names
    /// ([`needles`]), reading each one's process under `proc` (`/proc`).
    fn start(dir: PathBuf, game_match: &str, proc: PathBuf) -> Option<Self> {
        let (watch, seen) = Watch::new(dir, game_match, proc);
        let watch = Arc::new(watch);
        let (live, editions) = mpsc::channel();
        let events = Arc::clone(&watch);
        std::thread::Builder::new()
            .name("hypr-track".into())
            .spawn(move || track(&events))
            .ok()?;
        std::thread::Builder::new()
            .name("hypr-live".into())
            .spawn(move || relive(&watch, &editions))
            .ok()?;
        Some(Self { seen, live })
    }

    /// A tracker on a fake Hyprland's `dir`, reading processes under `proc`
    /// (`fake::proc_root`): for the GUI's tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn start_at(dir: PathBuf, game_match: &str, proc: PathBuf) -> Option<Self> {
        Self::start(dir, game_match, proc)
    }

    /// The live edition, as the overlay's session last heard it (`None`:
    /// not known). A change is looked at at once: which window is the
    /// game's, whether its workspace is on screen, and its monitor.
    pub fn set_live(&self, live: Option<Edition>) {
        let _ = self.live.send(live);
    }

    /// Everything seen since the last call, in order.
    pub fn drain(&self) -> Vec<Seen> {
        self.seen.try_iter().collect()
    }
}

/// Start the tracking threads. `None` when not running under Hyprland (no
/// instance signature, or its socket directory is gone) — the overlay then
/// simply never hides.
pub fn spawn(game_match: String) -> Option<Tracker> {
    Tracker::start(socket_dir()?, &game_match, PathBuf::from("/proc"))
}

pub fn socket_dir() -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let dir = PathBuf::from(runtime).join("hypr").join(sig);
    dir.join(".socket.sock").exists().then_some(dir)
}

/// Event lines that can change which workspaces are on screen or where the
/// game window lives. Everything else (focus, title spam, float toggles) is
/// noise not worth a recompute.
fn relevant(event: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "workspace",     // workspace>>, workspacev2>>: active workspace switched
        "focusedmon",    // focus crossed monitors, pulling a workspace forward
        "moveworkspace", // workspace dragged to another monitor
        "activespecial", // special (scratchpad) workspace shown or hidden
        "openwindow",    // the game may have just launched…
        "closewindow",   // …or quit…
        "movewindow",    // …or been sent to another workspace
        "monitoradded",
        "monitorremoved",
    ];
    PREFIXES.iter().any(|p| event.starts_with(p))
}

/// What both tracking threads share: where to look and for what, and what
/// the overlay was last told.
struct Watch {
    dir: PathBuf,
    needle: String,
    /// Where a window's process is read (`<proc>/<pid>/cmdline`).
    proc: PathBuf,
    /// Behind one lock, so the two threads never tell the overlay a
    /// transition out of order or twice.
    told: Mutex<Told>,
}

struct Told {
    tx: Sender<Seen>,
    /// The visibility last sent.
    visible: Option<bool>,
    /// The live edition last set.
    live: Option<Edition>,
    /// The live edition changed and its window's monitor is not told yet.
    home: bool,
}

impl Watch {
    /// Nothing told yet, and nothing known of the live edition.
    fn new(dir: PathBuf, game_match: &str, proc: PathBuf) -> (Self, Receiver<Seen>) {
        let (tx, seen) = mpsc::channel();
        let told = Told {
            tx,
            visible: None,
            live: None,
            home: false,
        };
        let watch = Self {
            dir,
            needle: game_match.to_lowercase(),
            proc,
            told: Mutex::new(told),
        };
        (watch, seen)
    }

    fn lock(&self) -> MutexGuard<'_, Told> {
        // A panic elsewhere holding the lock leaves plain data behind.
        self.told.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ask Hyprland again and tell the overlay what changed. `false`: the
    /// overlay hung up.
    fn recompute(&self, told: &mut Told) -> bool {
        let seen = look(&self.dir, &self.needle, told.live, &self.proc, told.home);
        if !push(&told.tx, &mut told.visible, seen.visible) {
            return false;
        }
        match seen.home {
            Some(monitor) => {
                told.home = false;
                told.tx.send(Seen::Monitor(monitor)).is_ok()
            }
            None => true,
        }
    }

    /// Hyprland out of reach: show the overlay. `false`: it hung up.
    fn fail_open(&self) -> bool {
        let told = &mut *self.lock();
        push(&told.tx, &mut told.visible, true)
    }
}

/// The event thread: a recompute on connecting and on every relevant event.
fn track(watch: &Watch) {
    loop {
        let Ok(events) = UnixStream::connect(watch.dir.join(".socket2.sock")) else {
            // Hyprland restarting (or gone): fail open and retry quietly.
            if !watch.fail_open() {
                return;
            }
            std::thread::sleep(Duration::from_secs(5));
            continue;
        };
        if !watch.recompute(&mut watch.lock()) {
            return;
        }
        for line in BufReader::new(events).lines() {
            let Ok(line) = line else { break };
            if relevant(&line) && !watch.recompute(&mut watch.lock()) {
                return;
            }
        }
        // The event stream dropped; show the overlay while reconnecting.
        if !watch.fail_open() {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// The edition thread: a recompute whenever the live edition changes, and
/// the end once the overlay drops its [`Tracker`].
fn relive(watch: &Watch, editions: &Receiver<Option<Edition>>) {
    for live in editions {
        let told = &mut *watch.lock();
        // A log naming no edition is Retail's, as the state reckons it.
        if told.live.map(Edition::counts_as) == live.map(Edition::counts_as) {
            continue;
        }
        told.live = live;
        told.home = live.is_some();
        if !watch.recompute(told) {
            return;
        }
    }
}

/// Send only transitions. `false` means the overlay hung up: time to exit.
fn push(tx: &Sender<Seen>, last: &mut Option<bool>, visible: bool) -> bool {
    if *last == Some(visible) {
        return true;
    }
    *last = Some(visible);
    tx.send(Seen::Visible(visible)).is_ok()
}

/// One request per connection — hyprctl's socket protocol.
fn query(dir: &Path, cmd: &str) -> Option<String> {
    let mut sock = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    sock.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    sock.set_write_timeout(Some(Duration::from_secs(2))).ok()?;
    sock.write_all(cmd.as_bytes()).ok()?;
    let mut reply = String::new();
    sock.read_to_string(&mut reply).ok()?;
    Some(reply)
}

/// The compositor-global cursor position. Surface-local pointer coordinates
/// go stale while a drag moves the surface under the cursor (a feedback
/// loop); this is the stable reference frame the overlay drags in.
pub fn cursor_pos(dir: &Path) -> Option<(i32, i32)> {
    parse_cursorpos(&query(dir, "cursorpos")?)
}

/// `cursorpos` replies `<x>, <y>`.
fn parse_cursorpos(reply: &str) -> Option<(i32, i32)> {
    let (x, y) = reply.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// The logical rectangle `(x, y, w, h)` of a monitor, in global coordinates.
pub type MonitorRect = (i32, i32, i32, i32);

/// The rectangle of the monitor containing the given global point. For
/// keeping the overlay on screen while dragging.
pub fn monitor_at(dir: &Path, point: (i32, i32)) -> Option<MonitorRect> {
    monitor_rects(&query(dir, "monitors")?)
        .into_iter()
        .find(|&(x, y, w, h)| (x..x + w).contains(&point.0) && (y..y + h).contains(&point.1))
}

/// The logical rectangle of the monitor named `name` (`DP-1`): how long
/// each of its edges is, for a surface that spans one.
pub fn monitor_named(dir: &Path, name: &str) -> Option<MonitorRect> {
    monitors(&query(dir, "monitors")?)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, rect)| rect)
}

/// Monitor rectangles from a plain-text `monitors` reply, in logical global
/// coordinates: the mode line (`<w>x<h>@<hz> at <x>x<y>`) divided by
/// `scale:`, width/height swapped by odd `transform:`s (90°/270°).
fn monitor_rects(monitors_reply: &str) -> Vec<(i32, i32, i32, i32)> {
    monitors(monitors_reply)
        .into_iter()
        .map(|(_, rect)| rect)
        .collect()
}

/// Each monitor of a `monitors` reply by its name (the `Monitor <name>
/// (ID n):` header), with its logical rectangle.
fn monitors(reply: &str) -> Vec<(String, MonitorRect)> {
    let mut out = Vec::new();
    // (name, x, y, mode_w, mode_h, scale, transform), flushed per block.
    type Block = (String, i32, i32, f32, f32, f32, u32);
    let mut name: Option<String> = None;
    let mut cur: Option<Block> = None;
    let flush = |cur: &mut Option<Block>, out: &mut Vec<(String, MonitorRect)>| {
        if let Some((name, x, y, mw, mh, scale, transform)) = cur.take() {
            let (w, h) = if transform % 2 == 1 {
                (mh, mw)
            } else {
                (mw, mh)
            };
            out.push((
                name,
                (x, y, (w / scale).round() as i32, (h / scale).round() as i32),
            ));
        }
    };
    for line in reply.lines() {
        if let Some(rest) = line.strip_prefix("Monitor ") {
            flush(&mut cur, &mut out);
            name = rest.split_whitespace().next().map(str::to_string);
            continue;
        }
        let t = line.trim();
        if let Some((dims, origin)) = t
            .split_once('@')
            .and_then(|(dims, rest)| Some((dims, rest.split_once(" at ")?.1)))
        {
            if let (Some((w, h)), Some((x, y))) = (parse_pair(dims, 'x'), parse_pair(origin, 'x')) {
                let n = name.clone().unwrap_or_default();
                cur = Some((n, x, y, w as f32, h as f32, 1.0, 0));
            }
        } else if let Some(cur) = cur.as_mut() {
            if let Some(s) = t.strip_prefix("scale:").and_then(|v| v.trim().parse().ok()) {
                cur.5 = s;
            } else if let Some(tr) = t
                .strip_prefix("transform:")
                .and_then(|v| v.trim().parse().ok())
            {
                cur.6 = tr;
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn parse_pair(s: &str, sep: char) -> Option<(i32, i32)> {
    let (a, b) = s.trim().split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// One look at Hyprland: whether the game's workspace is on some monitor
/// right now, and — when `home` asks and the game's window is the `live`
/// edition's by its process — the monitor that window is on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Look {
    visible: bool,
    home: Option<String>,
}

fn look(dir: &Path, needle: &str, live: Option<Edition>, proc: &Path, home: bool) -> Look {
    let open = Look {
        visible: true,
        home: None,
    };
    let Some(clients) = query(dir, "clients") else {
        return open;
    };
    let Some(game) = game_window(&clients, needle, live, proc) else {
        return open; // game not running: stay usable for log review
    };
    let home = (home && game.live)
        .then(|| workspace_monitor(&query(dir, "workspaces")?, game.ws))
        .flatten();
    let Some(monitors) = query(dir, "monitors") else {
        return Look { home, ..open };
    };
    Look {
        visible: on_screen_workspaces(&monitors).contains(&game.ws),
        home,
    }
}

/// The window needles a `game_match` names: one substring, or several
/// separated by `|` (one per edition's window, say), each lowercased and
/// otherwise taken as written — spaces are part of a needle, and a window
/// title holding a `|` cannot be named. An empty piece (`""`, a trailing
/// `|`, `||`) is no needle: it would match every window.
pub fn needles(game_match: &str) -> Vec<String> {
    game_match
        .split('|')
        .filter(|n| !n.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The game's window in a `clients` reply: its workspace, and whether it
/// was found by its process naming the live edition (not by score and
/// order alone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Game {
    ws: i32,
    live: bool,
}

/// [`game_window`] with no live edition: the best match, the first listed
/// on a tie. Where the overlay is born, before its session has said which
/// edition is live.
fn game_workspace(clients: &str, game_match: &str) -> Option<i32> {
    game_window(clients, game_match, None, Path::new("/proc")).map(|g| g.ws)
}

/// Find the game in a plain-text `clients` reply. Blocks start `Window
/// <addr> -> <title>:` with one indented `field: value` line each; each of
/// `game_match`'s needles ([`needles`]) is matched case-insensitively
/// against class, title, and their initial variants, and a window matching
/// any is the game's.
///
/// Every matching window is scored and the best wins, because a browser
/// tab titled "… - Item - World of Warcraft" matches the needle too and
/// would otherwise steer the overlay to its workspace: a class match
/// outranks a title match, a fullscreen window outranks a windowed one,
/// an exact title outranks a containing one, and the current title
/// outranks the initial one. On a tie the window listed first wins.
///
/// Two editions' clients open at once are two windows alike in class and
/// title; only their processes differ. So with the `live` edition known, a
/// matching window whose process (`pid:`, read under `proc`) names it by
/// its product folder ([`product_of`]) wins over every other, the best of
/// them if several do; with none known, or no window its — no `pid:`, a
/// command line that cannot be read or names no product — the best match
/// wins as above.
fn game_window(
    clients: &str,
    game_match: &str,
    live: Option<Edition>,
    proc: &Path,
) -> Option<Game> {
    let needles = needles(game_match);
    if needles.is_empty() {
        return None;
    }
    // Every matching window in the listed order: workspace, score, pid.
    let mut found: Vec<(i32, u32, Option<u32>)> = Vec::new();
    let (mut ws, mut score, mut fullscreen, mut pid) = (None::<i32>, 0u32, false, None::<u32>);
    let mut flush = |ws: Option<i32>, score: u32, fullscreen: bool, pid: Option<u32>| {
        if let Some(ws) = ws
            && score > 0
        {
            found.push((ws, score + u32::from(fullscreen) * 2, pid));
        }
    };
    for line in clients.lines() {
        if line.starts_with("Window ") {
            flush(ws, score, fullscreen, pid);
            (ws, score, fullscreen, pid) = (None, 0, false, None);
            continue;
        }
        let lower = line.trim().to_lowercase();
        if let Some(rest) = lower.strip_prefix("workspace:") {
            ws = rest
                .split_whitespace()
                .next()
                .and_then(|id| id.parse().ok());
        } else if let Some(rest) = lower.strip_prefix("fullscreen:") {
            fullscreen = rest.trim() != "0";
        } else if let Some(rest) = lower.strip_prefix("pid:") {
            pid = rest.trim().parse().ok().filter(|&p| p > 0);
        } else {
            for (key, weight) in [
                ("class:", 8),
                ("initialclass:", 8),
                ("title:", 4),
                ("initialtitle:", 1),
            ] {
                let Some(v) = lower.strip_prefix(key).map(str::trim) else {
                    continue;
                };
                for needle in needles.iter().filter(|n| v.contains(n.as_str())) {
                    // The game's own title IS the needle; a tab's merely
                    // contains it.
                    let exact = u32::from(v == needle) * 2;
                    score = score.max(weight + exact);
                }
            }
        }
    }
    flush(ws, score, fullscreen, pid);
    if let Some(live) = live.map(Edition::counts_as) {
        let theirs = found.iter().filter(|(_, _, pid)| {
            pid.and_then(|pid| process_edition(proc, pid))
                .is_some_and(|e| e.counts_as() == live)
        });
        if let Some(ws) = best(theirs) {
            return Some(Game { ws, live: true });
        }
    }
    best(found.iter()).map(|ws| Game { ws, live: false })
}

/// The workspace of the best scored of `windows` (workspace, score, pid),
/// the first listed on a tie.
fn best<'a>(windows: impl Iterator<Item = &'a (i32, u32, Option<u32>)>) -> Option<i32> {
    let mut best: Option<(u32, i32)> = None;
    for &(ws, total, _) in windows {
        if best.is_none_or(|(s, _)| total > s) {
            best = Some((total, ws));
        }
    }
    best.map(|(_, ws)| ws)
}

/// The edition process `pid`'s command line names by its product folder,
/// read under `proc` (`/proc`); `None` when it cannot be read or names none.
fn process_edition(proc: &Path, pid: u32) -> Option<Edition> {
    let cmdline = std::fs::read(proc.join(pid.to_string()).join("cmdline")).ok()?;
    product_of(&cmdline)
}

/// Workspace ids currently displayed, from a plain-text `monitors` reply:
/// every monitor's `active workspace`, plus its `special workspace` when one
/// is open (id 0 means none — specials have negative ids).
fn on_screen_workspaces(monitors: &str) -> Vec<i32> {
    monitors
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            let rest = t
                .strip_prefix("active workspace:")
                .or_else(|| t.strip_prefix("special workspace:"))?;
            let id: i32 = rest.split_whitespace().next()?.parse().ok()?;
            (id != 0).then_some(id)
        })
        .collect()
}

/// The NAME of the monitor the game's workspace lives on, for choosing the
/// layer surface's output at start. A layer-shell surface is born on one
/// output and never moves; `StartMode::Active` picks whichever monitor had
/// focus when the daemon spawned the overlay — the terminal's, on a
/// two-screen desk — so the game's monitor has to be asked for by name.
/// Answered from `workspaces` (every workspace names its monitor) rather
/// than `monitors`, so a game parked on a workspace that is not on screen
/// still resolves. `None` when Hyprland cannot say (no game window yet, no
/// match, no IPC): the caller falls back to the active output. No live
/// edition is known yet, so of two games' windows it is the first listed's;
/// the tracker tells the live one's monitor once the session names it
/// ([`Seen::Monitor`]), and the overlay opens again there.
pub fn game_monitor(dir: &Path, needle: &str) -> Option<String> {
    let ws = game_workspace(&query(dir, "clients")?, needle)?;
    workspace_monitor(&query(dir, "workspaces")?, ws)
}

/// `game_monitor`, retried until `deadline`: the daemon spawns the overlay
/// on the game PROCESS appearing, and under Proton the window that
/// `game_match` recognises maps seconds later. Polls every 500 ms; a hit
/// returns at once, the deadline returns `None`.
pub fn game_monitor_wait(dir: &Path, needle: &str, deadline: Instant) -> Option<String> {
    loop {
        if let Some(name) = game_monitor(dir, needle) {
            return Some(name);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The monitor named by the `workspace <id> (<name>) on monitor <mon>:`
/// header of the block for `ws` in a plain-text `workspaces` reply.
fn workspace_monitor(workspaces: &str, ws: i32) -> Option<String> {
    workspaces.lines().find_map(|line| {
        let rest = line.strip_prefix("workspace ")?;
        let id: i32 = rest.split_whitespace().next()?.parse().ok()?;
        if id != ws {
            return None;
        }
        let mon = rest.split(" on monitor ").nth(1)?;
        Some(mon.trim_end_matches(':').trim().to_string())
    })
}

/// A stand-in for Hyprland's IPC under a scratch directory, for tests: the
/// request socket answers `cursorpos`/`monitors`/`clients` from canned
/// (mutable) replies, one request per connection like the real thing, and
/// the event socket is a plain listener the test writes event lines into.
///
/// Test code compiled into the library under `test-support` (the GUI's
/// tests drive it), where clippy.toml's test exemptions do not reach: a
/// panic is still the failure mechanism here, so the same two are allowed.
#[cfg(any(test, feature = "test-support"))]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
pub mod fake {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, Once};
    use std::time::Duration;

    /// Two landscape monitors side by side at scale 1: DP-1 at the origin,
    /// DP-2 to its right. Workspace 9 (the game's) is on DP-2, 1 on DP-1.
    pub const MONITORS: &str = "\
Monitor DP-1 (ID 0):
	3440x1440@59.97300 at 0x0
	active workspace: 1 (1)
	special workspace: 0 ()
	scale: 1.00
	transform: 0

Monitor DP-2 (ID 1):
	1920x1080@144.00000 at 3440x0
	active workspace: 9 (9)
	special workspace: 0 ()
	scale: 1.00
	transform: 0
";

    /// The same two monitors with workspace 3 pulled up on DP-2: the game's
    /// workspace 9 is nowhere on screen.
    pub const MONITORS_GAME_HIDDEN: &str = "\
Monitor DP-1 (ID 0):
	3440x1440@59.97300 at 0x0
	active workspace: 1 (1)
	special workspace: 0 ()
	scale: 1.00
	transform: 0

Monitor DP-2 (ID 1):
	1920x1080@144.00000 at 3440x0
	active workspace: 3 (3)
	special workspace: 0 ()
	scale: 1.00
	transform: 0
";

    /// The game on workspace 9 plus a terminal on 1.
    pub const WORKSPACES: &str = "\
workspace 1 (1) on monitor DP-1:
	monitorID: 0
	windows: 2

workspace 9 (9) on monitor DP-2:
	monitorID: 1
	windows: 1
";

    pub const CLIENTS: &str = "\
Window 602419aa4810 -> World of Warcraft:
	mapped: 1
	workspace: 9 (9)
	class: steam_app_battlenet
	title: World of Warcraft
	pid: 1023268

Window 6024184fbab0 -> Ghostty:
	mapped: 1
	workspace: 1 (1)
	class: com.mitchellh.ghostty
	title: Ghostty
	pid: 4393
";

    /// Process-wide test environment, set once: config saves land under a
    /// scratch `XDG_CONFIG_HOME` (never the developer's real config), and
    /// `XDG_RUNTIME_DIR` + `HYPRLAND_INSTANCE_SIGNATURE` point
    /// [`super::socket_dir`] at a scratch path a test may populate with a
    /// [`FakeHypr`]. Returns the scratch root.
    pub fn test_env() -> PathBuf {
        static INIT: Once = Once::new();
        let root = scratch_root();
        INIT.call_once(|| {
            std::fs::create_dir_all(root.join("cfg")).unwrap();
            std::fs::create_dir_all(root.join("rt")).unwrap();
            // SAFETY: tests are the only readers of these variables in this
            // process, and every reader runs after this `Once`.
            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", root.join("cfg"));
                std::env::set_var("XDG_RUNTIME_DIR", root.join("rt"));
                std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", "fake");
            }
        });
        root
    }

    /// Short and per-process: unix socket paths are capped at ~108 bytes.
    /// A test that must leave the environment alone (the GUI's, beside
    /// others in threads) puts its [`FakeHypr::at`] under this.
    pub fn scratch_root() -> PathBuf {
        let base = std::env::temp_dir();
        let base = if base.as_os_str().len() > 40 {
            PathBuf::from("/tmp")
        } else {
            base
        };
        base.join(format!("wowdps-gui-{}", std::process::id()))
    }

    /// Where [`super::socket_dir`] resolves to under [`test_env`].
    pub fn env_socket_dir() -> PathBuf {
        test_env().join("rt").join("hypr").join("fake")
    }

    /// Each edition's client as both run at once: one invented install,
    /// the product folder and the executable's case apart.
    pub const RETAIL_EXE: &str = "Z:\\games\\World of Warcraft\\_retail_\\WoW.exe\0";
    pub const FOREVER_EXE: &str = "Z:\\games\\World of Warcraft\\_forever_\\Wow.exe\0";
    pub const RETAIL_PID: u32 = 5100;
    pub const FOREVER_PID: u32 = 4242;

    /// A game client's window as the compositor lists it with both games
    /// open: class `wow.exe`, titled `World of Warcraft`, fullscreen or
    /// not, neither focused. Only its workspace and process tell it apart.
    pub fn wow_window(addr: &str, ws: i32, pid: Option<i64>, fullscreen: bool) -> String {
        let pid = pid.map(|p| format!("\tpid: {p}\n")).unwrap_or_default();
        let fs = u8::from(fullscreen) * 2;
        format!(
            "Window {addr} -> World of Warcraft:\n\tmapped: 1\n\tworkspace: {ws} ({ws})\n\
             \tclass: wow.exe\n\ttitle: World of Warcraft\n\tinitialClass: wow.exe\n\
             \tinitialTitle: World of Warcraft\n\tfullscreen: {fs}\n{pid}\tfocusHistoryID: 3\n\n"
        )
    }

    /// Forever's window on workspace 9, fullscreen.
    pub fn forever_on_9() -> String {
        wow_window("7a1b2c3d4e50", 9, Some(FOREVER_PID.into()), true)
    }

    /// Retail's window on workspace 8, fullscreen.
    pub fn retail_on_8() -> String {
        wow_window("7a1b2c3d4e60", 8, Some(RETAIL_PID.into()), true)
    }

    /// The two monitors of [`MONITORS`] showing workspace `left` on DP-1
    /// and `right` on DP-2.
    pub fn monitors(left: i32, right: i32) -> String {
        format!(
            "Monitor DP-1 (ID 0):\n\t3440x1440@59.97300 at 0x0\n\
             \tactive workspace: {left} ({left})\n\tspecial workspace: 0 ()\n\
             \tscale: 1.00\n\ttransform: 0\n\n\
             Monitor DP-2 (ID 1):\n\t1920x1080@144.00000 at 3440x0\n\
             \tactive workspace: {right} ({right})\n\tspecial workspace: 0 ()\n\
             \tscale: 1.00\n\ttransform: 0\n"
        )
    }

    /// Retail's game on workspace 8 beside a terminal's 1, on DP-1;
    /// Forever's on 9, on DP-2.
    pub const WORKSPACES_TWO_GAMES: &str = "\
workspace 1 (1) on monitor DP-1:
	monitorID: 0

workspace 8 (8) on monitor DP-1:
	monitorID: 0

workspace 9 (9) on monitor DP-2:
	monitorID: 1
";

    /// A scratch `/proc` at `root` holding each process's command line,
    /// for a tracker to read windows' processes from.
    pub fn proc_root(root: &std::path::Path, procs: &[(u32, &str)]) -> PathBuf {
        let root = root.to_path_buf();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (pid, cmdline) in procs {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
        }
        root
    }

    /// Both games' processes, as [`proc_root`].
    pub fn both_games(root: &std::path::Path) -> PathBuf {
        proc_root(
            root,
            &[(RETAIL_PID, RETAIL_EXE), (FOREVER_PID, FOREVER_EXE)],
        )
    }

    pub struct FakeHypr {
        pub dir: PathBuf,
        pub cursor: Arc<Mutex<(i32, i32)>>,
        pub monitors: Arc<Mutex<String>>,
        pub clients: Arc<Mutex<String>>,
        #[allow(dead_code)]
        pub workspaces: Arc<Mutex<String>>,
        /// `.socket2.sock`: the test accepts the tracker's connection itself.
        events: UnixListener,
        stop: Arc<AtomicBool>,
    }

    impl FakeHypr {
        /// A fresh fake under its own scratch directory.
        pub fn start() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            Self::at(test_env().join(format!("h{n}")))
        }

        /// A fake serving from exactly `dir` (for [`super::socket_dir`]).
        pub fn at(dir: PathBuf) -> Self {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let requests = UnixListener::bind(dir.join(".socket.sock")).unwrap();
            requests.set_nonblocking(true).unwrap();
            let events = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
            let cursor = Arc::new(Mutex::new((100, 100)));
            let monitors = Arc::new(Mutex::new(MONITORS.to_string()));
            let clients = Arc::new(Mutex::new(CLIENTS.to_string()));
            let workspaces = Arc::new(Mutex::new(WORKSPACES.to_string()));
            let stop = Arc::new(AtomicBool::new(false));
            let ws = Arc::clone(&workspaces);
            let (c, m, cl, s) = (
                Arc::clone(&cursor),
                Arc::clone(&monitors),
                Arc::clone(&clients),
                Arc::clone(&stop),
            );
            std::thread::spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    match requests.accept() {
                        Ok((mut sock, _)) => {
                            let _ = sock.set_nonblocking(false);
                            let mut buf = [0u8; 256];
                            let n = sock.read(&mut buf).unwrap_or(0);
                            let cmd = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                            let reply = match cmd.as_str() {
                                "cursorpos" => {
                                    let (x, y) = *c.lock().unwrap();
                                    format!("{x}, {y}\n")
                                }
                                "monitors" => m.lock().unwrap().clone(),
                                "clients" => cl.lock().unwrap().clone(),
                                "workspaces" => ws.lock().unwrap().clone(),
                                _ => "unknown request".to_string(),
                            };
                            let _ = sock.write_all(reply.as_bytes());
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(2)),
                    }
                }
            });
            Self {
                dir,
                cursor,
                monitors,
                clients,
                workspaces,
                events,
                stop,
            }
        }

        pub fn set_cursor(&self, x: i32, y: i32) {
            *self.cursor.lock().unwrap() = (x, y);
        }

        pub fn set_monitors(&self, text: &str) {
            *self.monitors.lock().unwrap() = text.to_string();
        }

        pub fn set_workspaces(&self, text: &str) {
            *self.workspaces.lock().unwrap() = text.to_string();
        }

        pub fn set_clients(&self, text: &str) {
            *self.clients.lock().unwrap() = text.to_string();
        }

        /// Block until a tracker connects to the event socket; the returned
        /// stream is where the test writes event lines.
        pub fn accept_events(&self) -> UnixStream {
            self.events.accept().unwrap().0
        }
    }

    impl Drop for FakeHypr {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::{
        FOREVER_PID, FakeHypr, RETAIL_EXE, RETAIL_PID, forever_on_9, proc_root, retail_on_8,
        wow_window,
    };

    // Trimmed from a live `hyprctl clients` on Hyprland 0.4x.
    const CLIENTS: &str = "\
Window 602419aa4810 -> World of Warcraft:
	mapped: 1
	hidden: 0
	at: 1440,1000
	size: 3440,1440
	workspace: 9 (9)
	floating: 1
	monitor: 1
	class: steam_app_battlenet
	title: World of Warcraft
	initialClass: steam_app_battlenet
	initialTitle: World of Warcraft
	pid: 1023268

Window 6024184fbab0 -> Ghostty:
	mapped: 1
	workspace: 1 (1)
	class: com.mitchellh.ghostty
	title: Ghostty
	pid: 4393
";

    // Mirrors the live layout: DP-1 is portrait (transform 1), DP-2 is
    // landscape at scale 2. `availableModes` is the decoy line a naive
    // geometry matcher would trip over.
    const MONITORS: &str = "\
Monitor DP-1 (ID 0):
	3440x1440@59.97300 at 0x0
	active workspace: 1 (1)
	special workspace: 0 ()
	focused: no
	scale: 1.00
	transform: 1
	availableModes: 3440x1440@59.97Hz 1024x768@60.00Hz

Monitor DP-2 (ID 1):
	3440x1440@144.00000 at 1440x1000
	active workspace: 9 (9)
	special workspace: -98 (special:mus)
	focused: yes
	scale: 2.00
	transform: 0
";

    #[test]
    fn the_game_is_found_by_title_case_insensitively() {
        assert_eq!(game_workspace(CLIENTS, "world of warcraft"), Some(9));
        assert_eq!(game_workspace(CLIENTS, "steam_app_battlenet"), Some(9));
        assert_eq!(
            game_workspace(CLIENTS, "ghostty"),
            Some(1),
            "matches any field"
        );
        assert_eq!(game_workspace(CLIENTS, "factorio"), None);
    }

    /// `game_match` may name several windows, `|`-separated: a window
    /// matching any of them is the game's, ranked as one needle's would be.
    /// One needle, the default, matches as it always did.
    #[test]
    fn game_match_takes_several_windows() {
        assert_eq!(needles("World of Warcraft"), ["world of warcraft"]);
        assert_eq!(
            needles("World of Warcraft|Other Game|"),
            ["world of warcraft", "other game"]
        );
        // Spaces are part of a needle: a value is taken as written.
        assert_eq!(needles(" wow |x"), [" wow ", "x"]);
        // An empty piece is no needle: it would match every window.
        for empty in ["", "|", "||", "|||"] {
            assert!(needles(empty).is_empty(), "{empty:?}");
            assert_eq!(game_workspace(CLIENTS, empty), None, "{empty:?}");
        }
        assert_eq!(needles("a||b|"), ["a", "b"]);
        assert_eq!(game_workspace(CLIENTS, "||world of warcraft|"), Some(9));
        let other = "\
Window 70aa -> Other Game:
	mapped: 1
	workspace: 4 (4)
	class: other_game
	title: Other Game
";
        assert_eq!(
            game_workspace(other, "world of warcraft|other game"),
            Some(4)
        );
        assert_eq!(game_workspace(other, "world of warcraft"), None);
        // Both windows open: the needles that match decide which.
        let both = format!("{CLIENTS}{other}");
        assert_eq!(
            game_workspace(&both, "World of Warcraft|steam_app_battlenet"),
            Some(9)
        );
        assert_eq!(game_workspace(&both, "other_game|nothing"), Some(4));
    }

    #[test]
    fn a_match_in_the_last_block_is_not_dropped() {
        // Regression guard: blocks are finalized on the *next* header line,
        // so the final block needs its own flush after the loop.
        let only_game = CLIENTS
            .split_once("\nWindow 6024")
            .map(|(head, _)| head)
            .unwrap();
        assert_eq!(game_workspace(only_game, "warcraft"), Some(9));
    }

    #[test]
    fn on_screen_ids_include_open_specials_but_not_the_zero_sentinel() {
        assert_eq!(on_screen_workspaces(MONITORS), vec![1, 9, -98]);
    }

    #[test]
    fn the_game_monitor_is_the_one_its_workspace_names() {
        let ws = "workspace 1 (1) on monitor DP-1:\n\tmonitorID: 0\n\twindows: 2\n\n\
                  workspace 9 (9) on monitor DP-3:\n\tmonitorID: 1\n\twindows: 1\n\n\
                  workspace -98 (special:wow) on monitor DP-1:\n\tmonitorID: 0\n";
        assert_eq!(workspace_monitor(ws, 9).as_deref(), Some("DP-3"));
        assert_eq!(workspace_monitor(ws, 1).as_deref(), Some("DP-1"));
        assert_eq!(workspace_monitor(ws, -98).as_deref(), Some("DP-1"));
        assert_eq!(
            workspace_monitor(ws, 4),
            None,
            "a workspace that does not exist"
        );
    }

    #[test]
    fn game_monitor_waits_for_the_window_to_map_and_gives_up_at_the_deadline() {
        let fake = FakeHypr::start();
        // The fake's canned workspaces put 9 on DP-2.
        fake.set_clients("");
        let t = Instant::now();
        assert_eq!(
            game_monitor_wait(
                &fake.dir,
                "world of warcraft",
                t + Duration::from_millis(600)
            ),
            None,
            "no game window before the deadline"
        );
        assert!(
            t.elapsed() >= Duration::from_millis(500),
            "polled at least once more"
        );
        // The window maps while we wait.
        let fake2 = fake;
        let dir = fake2.dir.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            fake2.set_clients(CLIENTS);
            std::thread::sleep(Duration::from_secs(2));
            drop(fake2);
        });
        assert_eq!(
            game_monitor_wait(
                &dir,
                "world of warcraft",
                Instant::now() + Duration::from_secs(5)
            )
            .as_deref(),
            Some("DP-2")
        );
    }

    #[test]
    fn monitor_rects_respect_rotation_and_scale() {
        assert_eq!(
            monitor_rects(MONITORS),
            vec![(0, 0, 1440, 3440), (1440, 1000, 1720, 720)]
        );
    }

    #[test]
    fn cursorpos_replies_parse() {
        assert_eq!(parse_cursorpos("529, 3239\n"), Some((529, 3239)));
        assert_eq!(parse_cursorpos("-10, 4"), Some((-10, 4)));
        assert_eq!(parse_cursorpos("garbage"), None);
    }

    #[test]
    fn only_workspace_shaped_events_trigger_a_recompute() {
        for ev in [
            "workspace>>3",
            "workspacev2>>3,3",
            "focusedmon>>DP-1,1",
            "movewindowv2>>602419aa4810,9,9",
            "openwindow>>602419aa4810,9,steam_app_battlenet,World of Warcraft",
            "closewindow>>602419aa4810",
            "activespecialv2>>-98,special:mus,DP-2",
        ] {
            assert!(relevant(ev), "{ev}");
        }
        for ev in [
            "activewindow>>ghostty,Ghostty",
            "windowtitle>>1",
            "fullscreen>>1",
        ] {
            assert!(!relevant(ev), "{ev}");
        }
    }

    #[test]
    fn queries_are_one_request_per_connection_and_fail_soft() {
        let hypr = FakeHypr::start();
        assert_eq!(query(&hypr.dir, "cursorpos").as_deref(), Some("100, 100\n"));
        assert_eq!(
            query(&hypr.dir, "nonsense").as_deref(),
            Some("unknown request")
        );
        hypr.set_cursor(3500, 40);
        assert_eq!(cursor_pos(&hypr.dir), Some((3500, 40)));

        let gone = hypr.dir.join("nowhere");
        assert_eq!(query(&gone, "cursorpos"), None, "no socket: None");
        assert_eq!(cursor_pos(&gone), None);
    }

    #[test]
    fn monitor_at_resolves_the_monitor_under_a_global_point() {
        let hypr = FakeHypr::start();
        assert_eq!(monitor_at(&hypr.dir, (100, 100)), Some((0, 0, 3440, 1440)));
        assert_eq!(
            monitor_at(&hypr.dir, (3440, 0)),
            Some((3440, 0, 1920, 1080)),
            "the second monitor's origin belongs to it"
        );
        assert_eq!(
            monitor_at(&hypr.dir, (3439, 1439)),
            Some((0, 0, 3440, 1440))
        );
        assert_eq!(monitor_at(&hypr.dir, (-1, 5)), None, "off every monitor");
        assert_eq!(monitor_at(&hypr.dir, (4000, 1200)), None, "below DP-2");
        assert_eq!(monitor_at(&hypr.dir.join("nowhere"), (0, 0)), None);
    }

    /// One look with no live edition, no process read and no monitor
    /// asked for: the question the tracker always asked.
    fn visible_now(dir: &Path, needle: &str) -> bool {
        look(dir, needle, None, Path::new("/nonexistent"), false).visible
    }

    /// A watch on `dir` with no process to read, and its receiving end.
    fn watch(dir: &Path, needle: &str) -> (Arc<Watch>, Receiver<Seen>) {
        let (watch, seen) = Watch::new(dir.to_path_buf(), needle, PathBuf::from("/nonexistent"));
        (Arc::new(watch), seen)
    }

    #[test]
    fn visibility_is_the_game_workspace_being_on_some_monitor() {
        let hypr = FakeHypr::start();
        assert!(visible_now(&hypr.dir, "warcraft"), "ws 9 is up on DP-2");
        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        assert!(!visible_now(&hypr.dir, "warcraft"), "ws 9 is off screen");
        assert!(
            visible_now(&hypr.dir, "factorio"),
            "no game window: stay usable for log review"
        );
        hypr.set_clients("");
        assert!(visible_now(&hypr.dir, "warcraft"), "no clients at all");
        assert!(
            visible_now(&hypr.dir.join("nowhere"), "warcraft"),
            "no socket: fail open"
        );
    }

    #[test]
    fn push_sends_only_transitions_and_reports_a_hung_up_receiver() {
        let (tx, rx) = mpsc::channel();
        let mut last = None;
        assert!(push(&tx, &mut last, true));
        assert!(push(&tx, &mut last, true), "repeat: swallowed, still alive");
        assert!(push(&tx, &mut last, false));
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            vec![Seen::Visible(true), Seen::Visible(false)]
        );
        drop(rx);
        assert!(push(&tx, &mut last, false), "no send attempted, no verdict");
        assert!(
            !push(&tx, &mut last, true),
            "a transition finds the receiver gone"
        );
    }

    #[test]
    fn the_tracker_recomputes_on_relevant_events_and_exits_when_dropped() {
        let hypr = FakeHypr::start();
        let (w, rx) = watch(&hypr.dir, "warcraft");
        let tracker = std::thread::spawn(move || track(&w));
        let mut events = hypr.accept_events();
        let wait = Duration::from_secs(5);
        assert_eq!(
            rx.recv_timeout(wait),
            Ok(Seen::Visible(true)),
            "initial verdict on connect"
        );

        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        events
            .write_all(b"activewindow>>ghostty,Ghostty\n")
            .unwrap();
        events.write_all(b"windowtitle>>1\n").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(rx.try_recv().is_err(), "noise events never recompute");

        events.write_all(b"workspace>>3\n").unwrap();
        assert_eq!(
            rx.recv_timeout(wait),
            Ok(Seen::Visible(false)),
            "the game's workspace left"
        );
        events.write_all(b"focusedmon>>DP-1,1\n").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(rx.try_recv().is_err(), "same verdict: no push");

        hypr.set_monitors(fake::MONITORS);
        events.write_all(b"workspacev2>>9,9\n").unwrap();
        assert_eq!(rx.recv_timeout(wait), Ok(Seen::Visible(true)));
        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        events
            .write_all(b"movewindowv2>>602419aa4810,3,3\n")
            .unwrap();
        assert_eq!(rx.recv_timeout(wait), Ok(Seen::Visible(false)));

        // The overlay hangs up, then the stream drops: the reconnect path's
        // fail-open `true` is a transition, finds no receiver, and exits.
        drop(rx);
        drop(events);
        tracker.join().unwrap();
    }

    #[test]
    fn the_tracker_exits_on_the_first_transition_after_a_hangup() {
        // No event socket at all (Hyprland gone) and nobody listening: the
        // fail-open verdict finds no receiver and the thread ends at once —
        // no retry sleep.
        let dir = fake::test_env().join("no-hypr");
        std::fs::create_dir_all(&dir).unwrap();
        let (w, rx) = watch(&dir, "warcraft");
        drop(rx);
        track(&w);

        // Connected, then the overlay hangs up mid-stream: the next relevant
        // event's recompute is a transition and ends the thread.
        let hypr = FakeHypr::start();
        let (w, rx) = watch(&hypr.dir, "warcraft");
        let tracker = std::thread::spawn(move || track(&w));
        let mut events = hypr.accept_events();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(Seen::Visible(true))
        );
        drop(rx);
        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        events.write_all(b"workspace>>3\n").unwrap();
        tracker.join().unwrap();
    }

    #[test]
    fn spawn_needs_the_instance_socket_dir_from_the_environment() {
        let root = fake::test_env();
        let dir = fake::env_socket_dir();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(socket_dir(), None, "signature set but no socket dir yet");
        assert!(spawn("warcraft".into()).is_none());

        let hypr = FakeHypr::at(dir.clone());
        assert_eq!(socket_dir().as_deref(), Some(dir.as_path()));
        let tracker = spawn("World of Warcraft".into()).expect("tracking under the fake");
        let mut events = hypr.accept_events();
        let wait = Duration::from_secs(5);
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(true)));
        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        events.write_all(b"workspace>>3\n").unwrap();
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(false)));
        // Same exit choreography as the direct tracker test: hang up first,
        // then the stream — the fail-open `true` is a transition and finds
        // no receiver.
        drop(tracker);
        drop(events);
        drop(hypr);
        assert!(root.join("rt").exists());
    }

    #[test]
    fn a_browser_tab_naming_the_game_does_not_steal_the_workspace() {
        // Live regression: a wiki tab's initialTitle carried "World of
        // Warcraft" on workspace 2 and, listed first, steered the overlay
        // there while the fullscreen game sat on 9.
        let decoy = "\
Window 5f307a382400 -> Font of Venomous Rage - Item - World of Warcraft - Helium:
	mapped: 1
	workspace: 2 (2)
	class: helium
	title: Font of Venomous Rage - Item - World of Warcraft - Helium
	initialClass: helium
	initialTitle: Font of Venomous Rage - Item - World of Warcraft - Helium
	fullscreen: 0

";
        let clients = format!("{decoy}{CLIENTS}");
        assert_eq!(game_workspace(&clients, "world of warcraft"), Some(9));
        // Without the game, the tab is the best we have.
        assert_eq!(game_workspace(decoy, "world of warcraft"), Some(2));
    }

    /// Both games open, alike in class, title and fullscreen. With no live
    /// edition known they tie and the window listed first is the game's,
    /// as it always was; with one known, the window whose process names it
    /// by its product folder is, whichever is listed first.
    #[test]
    fn of_two_games_windows_the_live_editions_is_the_games() {
        let proc = fake::both_games(&fake::test_env().join("proc-both"));
        let default = crate::config::Config::default().game_match;
        let pick = |clients: &str, live| game_window(clients, &default, live, &proc);
        let game = |ws, live| Some(Game { ws, live });
        let forever_first = format!("{}{}", forever_on_9(), retail_on_8());
        let retail_first = format!("{}{}", retail_on_8(), forever_on_9());

        // The tie: order alone, today's rule, at spawn and with no edition.
        assert_eq!(pick(&forever_first, None), game(9, false));
        assert_eq!(pick(&retail_first, None), game(8, false));
        assert_eq!(game_workspace(&forever_first, &default), Some(9));
        assert_eq!(game_workspace(&retail_first, &default), Some(8));

        for clients in [&forever_first, &retail_first] {
            assert_eq!(pick(clients, Some(Edition::Forever)), game(9, true));
            assert_eq!(pick(clients, Some(Edition::Retail)), game(8, true));
            // A log naming no edition counts as Retail.
            assert_eq!(pick(clients, Some(Edition::Unknown(0))), game(8, true));
        }
        // An edition no window's process names: order, as with none.
        assert_eq!(
            pick(&forever_first, Some(Edition::Classic(2))),
            game(9, false)
        );
        assert_eq!(
            pick(&retail_first, Some(Edition::Classic(2))),
            game(8, false)
        );

        // The live game windowed beside the other's fullscreen one: still
        // the live one's. The process outranks every other score.
        let windowed = format!(
            "{}{}",
            forever_on_9(),
            wow_window("7a1b2c3d4e60", 8, Some(RETAIL_PID.into()), false)
        );
        assert_eq!(pick(&windowed, Some(Edition::Retail)), game(8, true));
        assert_eq!(pick(&windowed, None), game(9, false));

        // A browser tab naming the game, listed first, never wins either.
        let decoy = "\
Window 5f307a382400 -> Some Item - World of Warcraft - Browser:
	mapped: 1
	workspace: 2 (2)
	class: browser
	title: Some Item - World of Warcraft - Browser
	fullscreen: 0
	pid: 4393
";
        let with_tab = format!("{decoy}{forever_first}");
        assert_eq!(pick(&with_tab, Some(Edition::Retail)), game(8, true));
        assert_eq!(pick(&with_tab, None), game(9, false));

        // One game window: it is the game's whichever edition is live, and
        // told apart by its process only when it is the live one's.
        assert_eq!(pick(&retail_on_8(), Some(Edition::Forever)), game(8, false));
        assert_eq!(pick(&retail_on_8(), Some(Edition::Retail)), game(8, true));
        assert_eq!(pick(&retail_on_8(), None), game(8, false));
        let _ = std::fs::remove_dir_all(&proc);
    }

    /// A window whose process cannot be named — its command line unread
    /// (the process gone, another user's), no `pid:` line, a pid Hyprland
    /// gives as -1 or 0, a client of no known product — is never the live
    /// edition's: the live game's window still wins when its own process
    /// names it, and with no window naming it, order decides as before.
    #[test]
    fn a_window_whose_process_cannot_be_read_falls_back_to_order() {
        // Forever's process is gone from /proc; Retail's reads.
        let proc = proc_root(
            &fake::test_env().join("proc-unread"),
            &[(RETAIL_PID, RETAIL_EXE)],
        );
        let default = crate::config::Config::default().game_match;
        let pick = |clients: &str, live| game_window(clients, &default, live, &proc);
        let game = |ws, live| Some(Game { ws, live });
        let forever_first = format!("{}{}", forever_on_9(), retail_on_8());
        assert_eq!(pick(&forever_first, Some(Edition::Forever)), game(9, false));
        assert_eq!(pick(&forever_first, Some(Edition::Retail)), game(8, true));

        for pid in [None, Some(-1), Some(0)] {
            let clients = format!(
                "{}{}",
                wow_window("7a1b2c3d4e60", 8, pid, true),
                forever_on_9()
            );
            assert_eq!(
                pick(&clients, Some(Edition::Retail)),
                game(8, false),
                "{pid:?}"
            );
        }

        // A command line naming no product folder, and an empty one.
        let odd = proc_root(
            &fake::test_env().join("proc-odd"),
            &[(RETAIL_PID, "Z:\\wow\\Wow.exe\0"), (FOREVER_PID, "")],
        );
        let retail_first = format!("{}{}", retail_on_8(), forever_on_9());
        assert_eq!(
            game_window(&retail_first, &default, Some(Edition::Forever), &odd),
            game(8, false)
        );
        let _ = std::fs::remove_dir_all(&proc);
        let _ = std::fs::remove_dir_all(&odd);
    }

    /// Both games open on two monitors: Retail's on 8 (DP-1), Forever's on
    /// 9 (DP-2), Forever's listed first. The tracker starts on today's rule;
    /// each change of live edition is looked at at once — visibility by
    /// that edition's workspace, and its monitor told once; a live edition
    /// whose window has not mapped yet is told when it maps; and no live
    /// edition is today's rule again.
    #[test]
    fn the_tracker_follows_the_live_editions_window() {
        use fake::monitors;
        let hypr = FakeHypr::start();
        hypr.set_workspaces(fake::WORKSPACES_TWO_GAMES);
        hypr.set_clients(&format!("{}{}", forever_on_9(), retail_on_8()));
        // A terminal up on DP-1, Forever's game on DP-2.
        hypr.set_monitors(&monitors(1, 9));
        let proc = fake::both_games(&fake::test_env().join("proc-tracker"));
        let tracker = Tracker::start(hypr.dir.clone(), "World of Warcraft", proc.clone()).unwrap();
        let mut events = hypr.accept_events();
        let wait = Duration::from_secs(5);
        let quiet = |tracker: &Tracker| {
            std::thread::sleep(Duration::from_millis(150));
            tracker.drain()
        };
        // No live edition: the first listed, Forever's, which is shown.
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(true)));

        // Retail is live: its workspace is off screen, and it is on DP-1.
        tracker.set_live(Some(Edition::Retail));
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(false)));
        assert_eq!(
            tracker.seen.recv_timeout(wait),
            Ok(Seen::Monitor("DP-1".into()))
        );
        tracker.set_live(Some(Edition::Retail));
        assert_eq!(quiet(&tracker), [], "the same edition again: nothing new");
        tracker.set_live(Some(Edition::Unknown(0)));
        assert_eq!(quiet(&tracker), [], "a log naming none is Retail's");

        // Its workspace comes up: shown, and the monitor is not told again.
        hypr.set_monitors(&monitors(8, 9));
        events.write_all(b"workspace>>8\n").unwrap();
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(true)));
        hypr.set_monitors(&monitors(8, 3));
        events.write_all(b"workspace>>3\n").unwrap();
        assert_eq!(
            quiet(&tracker),
            [],
            "Forever's workspace is not the game's now"
        );

        // Forever is live: its workspace 9 is off screen, on DP-2.
        tracker.set_live(Some(Edition::Forever));
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(false)));
        assert_eq!(
            tracker.seen.recv_timeout(wait),
            Ok(Seen::Monitor("DP-2".into()))
        );

        // No live edition: the first listed again, Forever's, still hidden.
        tracker.set_live(None);
        assert_eq!(quiet(&tracker), []);

        // Forever's client quits and Retail's is live alone: shown on 8.
        hypr.set_monitors(&monitors(8, 9));
        hypr.set_clients(&retail_on_8());
        events.write_all(b"closewindow>>7a1b2c3d4e50\n").unwrap();
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(true)));
        tracker.set_live(Some(Edition::Retail));
        assert_eq!(
            tracker.seen.recv_timeout(wait),
            Ok(Seen::Monitor("DP-1".into()))
        );

        // Forever goes live before its window maps: nothing to tell yet,
        // the one window still the game's; told once it maps.
        tracker.set_live(Some(Edition::Forever));
        assert_eq!(quiet(&tracker), []);
        hypr.set_clients(&format!("{}{}", retail_on_8(), forever_on_9()));
        hypr.set_monitors(&monitors(8, 3));
        events
            .write_all(b"openwindow>>7a1b2c3d4e50,9,wow.exe,World of Warcraft\n")
            .unwrap();
        assert_eq!(tracker.seen.recv_timeout(wait), Ok(Seen::Visible(false)));
        assert_eq!(
            tracker.seen.recv_timeout(wait),
            Ok(Seen::Monitor("DP-2".into()))
        );
        events.write_all(b"workspace>>3\n").unwrap();
        assert_eq!(quiet(&tracker), [], "told once per change");

        drop(tracker);
        drop(events);
        let _ = std::fs::remove_dir_all(&proc);
    }

    /// The edition thread ends when the overlay drops its sender, and on
    /// the first thing it would tell an overlay that has hung up.
    #[test]
    fn the_edition_thread_ends_with_the_overlay() {
        let hypr = FakeHypr::start();
        let (w, _rx) = watch(&hypr.dir, "warcraft");
        let (live, editions) = mpsc::channel();
        let thread = std::thread::spawn(move || relive(&w, &editions));
        live.send(None).unwrap();
        drop(live);
        thread.join().unwrap();

        let (w, rx) = watch(&hypr.dir, "warcraft");
        let (live, editions) = mpsc::channel();
        let thread = std::thread::spawn(move || relive(&w, &editions));
        drop(rx);
        // The first verdict is a transition and finds no one.
        live.send(Some(Edition::Retail)).unwrap();
        thread.join().unwrap();
        drop(live);
    }

    /// Forever's client as the compositor lists it — a native Wayland
    /// window of class `wow.exe` titled `World of Warcraft`, fullscreen — is
    /// the game's under the default `game_match`, by its exact title, and
    /// a browser tab naming the game never outranks it.
    #[test]
    fn the_default_match_finds_forevers_window() {
        let forever = "\
Window 7a1b2c3d4e50 -> World of Warcraft:
	mapped: 1
	hidden: 0
	workspace: 7 (7)
	floating: 0
	monitor: 1
	class: wow.exe
	title: World of Warcraft
	initialClass: wow.exe
	initialTitle: World of Warcraft
	fullscreen: 2
	pid: 4242
";
        let default = crate::config::Config::default().game_match;
        assert_eq!(game_workspace(forever, &default), Some(7));
        let decoy = "\
Window 5f307a382400 -> Some Item - World of Warcraft - Browser:
	mapped: 1
	workspace: 2 (2)
	class: browser
	title: Some Item - World of Warcraft - Browser
	fullscreen: 0
";
        assert_eq!(
            game_workspace(&format!("{decoy}{forever}"), &default),
            Some(7)
        );
    }

    /// Both games open at once, as the compositor lists them: two native
    /// Wayland windows alike in all but their process — class `wow.exe`,
    /// titled `World of Warcraft`, fullscreen — Forever's on workspace 9,
    /// Retail's on 8, neither focused. The match alone cannot tell them
    /// apart: with no live edition known (the overlay's spawn, before its
    /// session names one) the window listed first wins. With one known, the
    /// window whose process's product folder names it wins, whichever is
    /// listed first, and the overlay shows, hides and places itself by it.
    #[test]
    fn two_games_windows_are_told_apart_by_the_live_editions_process() {
        // Neither focused: some other window holds focus history 0.
        let window = |addr: &str, ws: i32, pid: u32, focus: u32| {
            format!(
                "\
Window {addr} -> World of Warcraft:
	mapped: 1
	workspace: {ws} ({ws})
	class: wow.exe
	title: World of Warcraft
	initialClass: wow.exe
	initialTitle: World of Warcraft
	fullscreen: 2
	pid: {pid}
	focusHistoryID: {focus}
"
            )
        };
        let default = crate::config::Config::default().game_match;
        let forever = window("7a1b2c3d4e50", 9, 4242, 1);
        let retail = window("7a1b2c3d4e60", 8, 5100, 2);
        assert_eq!(
            game_workspace(&format!("{forever}{retail}"), &default),
            Some(9)
        );
        assert_eq!(
            game_workspace(&format!("{retail}{forever}"), &default),
            Some(8)
        );
        let proc = fake::both_games(&fake::test_env().join("proc-order"));
        for clients in [format!("{forever}{retail}"), format!("{retail}{forever}")] {
            let ws = |live| game_window(&clients, &default, Some(live), &proc).map(|g| g.ws);
            assert_eq!(ws(Edition::Forever), Some(9));
            assert_eq!(ws(Edition::Retail), Some(8));
        }
        let _ = std::fs::remove_dir_all(&proc);
    }
}
