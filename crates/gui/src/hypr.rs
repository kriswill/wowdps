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
use std::time::Duration;

/// Start the tracking thread. `None` when not running under Hyprland (no
/// instance signature, or its socket directory is gone) — the overlay then
/// simply never hides.
pub fn spawn(game_match: String) -> Option<Receiver<bool>> {
    let dir = socket_dir()?;
    let (tx, rx) = mpsc::channel();
    let needle = game_match.to_lowercase();
    std::thread::Builder::new()
        .name("hypr-track".into())
        .spawn(move || track(&dir, &needle, &tx))
        .ok()?;
    Some(rx)
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

fn track(dir: &Path, needle: &str, tx: &Sender<bool>) {
    let mut last = None;
    loop {
        let Ok(events) = UnixStream::connect(dir.join(".socket2.sock")) else {
            // Hyprland restarting (or gone): fail open and retry quietly.
            if !push(tx, &mut last, true) {
                return;
            }
            std::thread::sleep(Duration::from_secs(5));
            continue;
        };
        if !push(tx, &mut last, visible_now(dir, needle)) {
            return;
        }
        for line in BufReader::new(events).lines() {
            let Ok(line) = line else { break };
            if relevant(&line) && !push(tx, &mut last, visible_now(dir, needle)) {
                return;
            }
        }
        // The event stream dropped; show the overlay while reconnecting.
        if !push(tx, &mut last, true) {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Send only transitions. `false` means the overlay hung up: time to exit.
fn push(tx: &Sender<bool>, last: &mut Option<bool>, visible: bool) -> bool {
    if *last == Some(visible) {
        return true;
    }
    *last = Some(visible);
    tx.send(visible).is_ok()
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

/// Monitor rectangles from a plain-text `monitors` reply, in logical global
/// coordinates: the mode line (`<w>x<h>@<hz> at <x>x<y>`) divided by
/// `scale:`, width/height swapped by odd `transform:`s (90°/270°).
fn monitor_rects(monitors: &str) -> Vec<(i32, i32, i32, i32)> {
    let mut rects = Vec::new();
    // (x, y, mode_w, mode_h, scale, transform), flushed per `Monitor` block.
    let mut cur: Option<(i32, i32, f32, f32, f32, u32)> = None;
    let flush = |cur: &mut Option<(i32, i32, f32, f32, f32, u32)>,
                 rects: &mut Vec<(i32, i32, i32, i32)>| {
        if let Some((x, y, mw, mh, scale, transform)) = cur.take() {
            let (w, h) = if transform % 2 == 1 {
                (mh, mw)
            } else {
                (mw, mh)
            };
            rects.push((x, y, (w / scale).round() as i32, (h / scale).round() as i32));
        }
    };
    for line in monitors.lines() {
        if line.starts_with("Monitor ") {
            flush(&mut cur, &mut rects);
            continue;
        }
        let t = line.trim();
        if let Some((dims, origin)) = t
            .split_once('@')
            .and_then(|(dims, rest)| Some((dims, rest.split_once(" at ")?.1)))
        {
            if let (Some((w, h)), Some((x, y))) = (parse_pair(dims, 'x'), parse_pair(origin, 'x')) {
                cur = Some((x, y, w as f32, h as f32, 1.0, 0));
            }
        } else if let Some(cur) = cur.as_mut() {
            if let Some(s) = t.strip_prefix("scale:").and_then(|v| v.trim().parse().ok()) {
                cur.4 = s;
            } else if let Some(tr) = t
                .strip_prefix("transform:")
                .and_then(|v| v.trim().parse().ok())
            {
                cur.5 = tr;
            }
        }
    }
    flush(&mut cur, &mut rects);
    rects
}

fn parse_pair(s: &str, sep: char) -> Option<(i32, i32)> {
    let (a, b) = s.trim().split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// Is the game's workspace on some monitor right now?
fn visible_now(dir: &Path, needle: &str) -> bool {
    let Some(clients) = query(dir, "clients") else {
        return true;
    };
    let Some(ws) = game_workspace(&clients, needle) else {
        return true; // game not running: stay usable for log review
    };
    let Some(monitors) = query(dir, "monitors") else {
        return true;
    };
    on_screen_workspaces(&monitors).contains(&ws)
}

/// Find the game in a plain-text `clients` reply and return its workspace
/// id. Blocks start `Window <addr> -> <title>:` with one indented
/// `field: value` line each; the needle is matched case-insensitively
/// against class, title, and their initial variants.
fn game_workspace(clients: &str, needle: &str) -> Option<i32> {
    let mut ws: Option<i32> = None;
    let mut is_game = false;
    for line in clients.lines() {
        if line.starts_with("Window ") {
            if is_game && ws.is_some() {
                return ws;
            }
            ws = None;
            is_game = false;
            continue;
        }
        let lower = line.trim().to_lowercase();
        if let Some(rest) = lower.strip_prefix("workspace:") {
            ws = rest
                .split_whitespace()
                .next()
                .and_then(|id| id.parse().ok());
        } else {
            for key in ["class:", "title:", "initialclass:", "initialtitle:"] {
                if lower.strip_prefix(key).is_some_and(|v| v.contains(needle)) {
                    is_game = true;
                }
            }
        }
    }
    if is_game { ws } else { None }
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

/// A stand-in for Hyprland's IPC under a scratch directory, for tests: the
/// request socket answers `cursorpos`/`monitors`/`clients` from canned
/// (mutable) replies, one request per connection like the real thing, and
/// the event socket is a plain listener the test writes event lines into.
#[cfg(test)]
pub(crate) mod fake {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, Once};
    use std::time::Duration;

    /// Two landscape monitors side by side at scale 1: DP-1 at the origin,
    /// DP-2 to its right. Workspace 9 (the game's) is on DP-2, 1 on DP-1.
    pub(crate) const MONITORS: &str = "\
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
    pub(crate) const MONITORS_GAME_HIDDEN: &str = "\
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
    pub(crate) const CLIENTS: &str = "\
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
    pub(crate) fn test_env() -> PathBuf {
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
    fn scratch_root() -> PathBuf {
        let base = std::env::temp_dir();
        let base = if base.as_os_str().len() > 40 {
            PathBuf::from("/tmp")
        } else {
            base
        };
        base.join(format!("wowdps-gui-{}", std::process::id()))
    }

    /// Where [`super::socket_dir`] resolves to under [`test_env`].
    pub(crate) fn env_socket_dir() -> PathBuf {
        test_env().join("rt").join("hypr").join("fake")
    }

    pub(crate) struct FakeHypr {
        pub(crate) dir: PathBuf,
        pub(crate) cursor: Arc<Mutex<(i32, i32)>>,
        pub(crate) monitors: Arc<Mutex<String>>,
        pub(crate) clients: Arc<Mutex<String>>,
        /// `.socket2.sock`: the test accepts the tracker's connection itself.
        events: UnixListener,
        stop: Arc<AtomicBool>,
    }

    impl FakeHypr {
        /// A fresh fake under its own scratch directory.
        pub(crate) fn start() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            Self::at(test_env().join(format!("h{n}")))
        }

        /// A fake serving from exactly `dir` (for [`super::socket_dir`]).
        pub(crate) fn at(dir: PathBuf) -> Self {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let requests = UnixListener::bind(dir.join(".socket.sock")).unwrap();
            requests.set_nonblocking(true).unwrap();
            let events = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
            let cursor = Arc::new(Mutex::new((100, 100)));
            let monitors = Arc::new(Mutex::new(MONITORS.to_string()));
            let clients = Arc::new(Mutex::new(CLIENTS.to_string()));
            let stop = Arc::new(AtomicBool::new(false));
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
                events,
                stop,
            }
        }

        pub(crate) fn set_cursor(&self, x: i32, y: i32) {
            *self.cursor.lock().unwrap() = (x, y);
        }

        pub(crate) fn set_monitors(&self, text: &str) {
            *self.monitors.lock().unwrap() = text.to_string();
        }

        pub(crate) fn set_clients(&self, text: &str) {
            *self.clients.lock().unwrap() = text.to_string();
        }

        /// Block until a tracker connects to the event socket; the returned
        /// stream is where the test writes event lines.
        pub(crate) fn accept_events(&self) -> UnixStream {
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
    use fake::FakeHypr;

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
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![true, false]);
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
        let (tx, rx) = mpsc::channel();
        let dir = hypr.dir.clone();
        let tracker = std::thread::spawn(move || track(&dir, "warcraft", &tx));
        let mut events = hypr.accept_events();
        let wait = Duration::from_secs(5);
        assert_eq!(
            rx.recv_timeout(wait),
            Ok(true),
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
            Ok(false),
            "the game's workspace left"
        );
        events.write_all(b"focusedmon>>DP-1,1\n").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(rx.try_recv().is_err(), "same verdict: no push");

        hypr.set_monitors(fake::MONITORS);
        events.write_all(b"workspacev2>>9,9\n").unwrap();
        assert_eq!(rx.recv_timeout(wait), Ok(true));
        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        events
            .write_all(b"movewindowv2>>602419aa4810,3,3\n")
            .unwrap();
        assert_eq!(rx.recv_timeout(wait), Ok(false));

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
        let (tx, rx) = mpsc::channel::<bool>();
        drop(rx);
        track(&dir, "warcraft", &tx);

        // Connected, then the overlay hangs up mid-stream: the next relevant
        // event's recompute is a transition and ends the thread.
        let hypr = FakeHypr::start();
        let (tx, rx) = mpsc::channel();
        let dir = hypr.dir.clone();
        let tracker = std::thread::spawn(move || track(&dir, "warcraft", &tx));
        let mut events = hypr.accept_events();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
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
        let rx = spawn("World of Warcraft".into()).expect("tracking under the fake");
        let mut events = hypr.accept_events();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(true));
        hypr.set_monitors(fake::MONITORS_GAME_HIDDEN);
        events.write_all(b"workspace>>3\n").unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)), Ok(false));
        // Same exit choreography as the direct tracker test: hang up first,
        // then the stream — the fail-open `true` is a transition and finds
        // no receiver.
        drop(rx);
        drop(events);
        drop(hypr);
        assert!(root.join("rt").exists());
    }
}
