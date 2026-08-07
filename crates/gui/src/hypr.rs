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

#[cfg(test)]
mod tests {
    use super::*;

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
}
