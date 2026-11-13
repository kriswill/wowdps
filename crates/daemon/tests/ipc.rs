//! Integration suite: real daemons on temp sockets, real fixture data, real
//! protocol frames. Every test drives `wowdps_daemon::run` the way a client
//! binary would.

#[path = "../../core/tests/real_log/mod.rs"]
mod real_log;

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::meter::meter_from_lines;
use wowdps_core::model::{SegmentId, SegmentKind, View};
use wowdps_core::tail::SourceSpec;
use wowdps_daemon::{DaemonOptions, run};
use wowdps_proto::{
    ClientKind, ClientMsg, Cursor, DaemonMsg, LoadError, PROTO_VERSION, SegmentRef, wire,
};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(10);

// ---- scaffolding ------------------------------------------------------------

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("wowdps-ipc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        // The panic bans reach helpers outside `#[test]` fns, so scaffolding
        // failures assert rather than unwrap.
        assert!(std::fs::create_dir_all(&p).is_ok(), "mkdir {p:?}");
        Temp(p)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Daemon {
    socket: PathBuf,
    done: mpsc::Receiver<std::io::Result<()>>,
}

fn options(tmp: &Temp, source: SourceSpec) -> DaemonOptions {
    DaemonOptions {
        socket: tmp.join("test.sock"),
        lockfile: tmp.join("test.lock"),
        source,
        editions: Default::default(),
        linger: true,
        idle_grace: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        version: "test".to_string(),
        cache_dir: None,
        game_pattern: None,
        loader_workers: 2,
        auto_overlay: false,
        overlay_exit_grace: Duration::ZERO,
        gui_bin: None,
        history: None,
    }
}

fn start(opts: DaemonOptions) -> Daemon {
    let socket = opts.socket.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(opts));
    });
    let deadline = Instant::now() + DEADLINE;
    loop {
        // The socket's vnode appears at bind(); listen() follows. A connect
        // in that gap is refused, so readiness is a connect, not exists().
        if UnixStream::connect(&socket).is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "daemon never bound {socket:?}");
        thread::sleep(Duration::from_millis(5));
    }
    Daemon { socket, done: rx }
}

struct Client {
    stream: UnixStream,
}

impl Client {
    fn connect(socket: &Path) -> Self {
        let deadline = Instant::now() + DEADLINE;
        let stream = loop {
            match UnixStream::connect(socket) {
                Ok(s) => break s,
                Err(e) => {
                    assert!(Instant::now() < deadline, "connect {socket:?}: {e}");
                    thread::sleep(Duration::from_millis(5));
                }
            }
        };
        assert!(
            stream.set_read_timeout(Some(DEADLINE)).is_ok(),
            "set read timeout"
        );
        Client { stream }
    }

    fn send(&mut self, msg: &ClientMsg) {
        assert!(self.stream.write_all(&msg.encode()).is_ok(), "send {msg:?}");
    }

    fn recv(&mut self) -> DaemonMsg {
        let frame = wire::read_frame(&mut self.stream);
        assert!(frame.is_ok(), "daemon frame");
        let (tag, body) = frame.unwrap_or_default();
        let msg = DaemonMsg::decode(tag, &body);
        assert!(msg.is_ok(), "valid daemon message (tag {tag})");
        msg.unwrap_or(DaemonMsg::Fatal(String::new()))
    }

    fn hello(socket: &Path) -> Self {
        Self::hello_as(socket, ClientKind::Tui)
    }

    fn hello_as(socket: &Path, kind: ClientKind) -> Self {
        let mut c = Self::connect(socket);
        c.send(&ClientMsg::Hello {
            proto: PROTO_VERSION,
            client: kind,
            pid: std::process::id(),
        });
        let ack = c.recv();
        let proto = match &ack {
            DaemonMsg::HelloAck { proto, .. } => Some(*proto),
            _ => None,
        };
        assert_eq!(proto, Some(PROTO_VERSION), "expected HelloAck, got {ack:?}");
        c
    }

    fn watch_live(&mut self, view: View, drill: Option<&str>) {
        self.send(&ClientMsg::watch(Cursor::Segment {
            segment: SegmentRef::Live,
            view,
            top_n: None,
            drill: drill.map(str::to_string),
            death: None,
            spell: None,
            range: None,
        }));
    }

    fn watch_id(&mut self, id: SegmentId, view: View) {
        self.send(&ClientMsg::watch(Cursor::Segment {
            segment: SegmentRef::Id(id),
            view,
            top_n: None,
            drill: None,
            death: None,
            spell: None,
            range: None,
        }));
    }

    fn watch_list(&mut self) {
        self.send(&ClientMsg::watch(Cursor::List));
    }

    /// Read until `pred` accepts a message; earlier messages go to `seen`.
    fn recv_until(
        &mut self,
        seen: &mut Vec<DaemonMsg>,
        pred: impl Fn(&DaemonMsg) -> bool,
    ) -> DaemonMsg {
        let deadline = Instant::now() + DEADLINE;
        loop {
            assert!(Instant::now() < deadline, "gave up waiting; saw {seen:#?}");
            let msg = self.recv();
            if pred(&msg) {
                return msg;
            }
            seen.push(msg);
        }
    }
}

/// A minimal advanced-format damage line the parser and scanner both accept.
fn hit(min: u32, sec: u32) -> String {
    format!(
        "7/27/2026 21:{min:02}:{sec:02}.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n"
    )
}

fn append(path: &Path, text: &str) {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);
    assert!(f.is_ok(), "open {path:?}");
    if let Ok(mut f) = f {
        assert!(f.write_all(text.as_bytes()).is_ok(), "append to {path:?}");
    }
}

/// A same-pull damage line from a *second* player: advances the tail and the
/// open segment's clock without touching Player-1-A's rows or breakdown.
fn probe_hit(sec: u32) -> String {
    format!(
        "7/27/2026 21:00:{sec:02}.000-7  SPELL_DAMAGE,Player-1-B,\"Bo-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,133,\"Fireball\",4,700,700,0,0,0,0,0,nil,nil\n"
    )
}

/// Wait (deadline-polled) until the daemon's tailer has drained the backlog
/// (`CaughtUp`). No wire message announces it, so this probes for it: a line
/// appended *after* CaughtUp counts as fresh combat, which flips the segment
/// list's `active` verdict — and every probe changes the open segment's
/// duration, so the changed-only pusher answers each one. Probes are
/// `probe_hit` lines, one per second from `sec` up to (never reaching)
/// `limit`, all inside the open pull, so segmentation is untouched.
fn wait_caught_up(socket: &Path, log: &Path, mut sec: u32, limit: u32) {
    let mut c = Client::hello(socket);
    c.watch_list();
    let mut seen = Vec::new();
    c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::SegmentList { .. }));
    let deadline = Instant::now() + DEADLINE;
    loop {
        assert!(
            Instant::now() < deadline && sec < limit,
            "tailer never caught up; saw {seen:#?}"
        );
        append(log, &probe_hit(sec));
        sec += 1;
        let list = c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::SegmentList { .. }));
        if matches!(list, DaemonMsg::SegmentList { active: true, .. }) {
            return;
        }
    }
}

fn fixture_lines() -> Vec<String> {
    let text = std::fs::read_to_string(FIXTURE);
    assert!(text.is_ok(), "{FIXTURE}: unreadable fixture");
    text.unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

// ---- tests ------------------------------------------------------------------

#[test]
fn handshake_acks_with_the_daemon_version() {
    let tmp = Temp::new("hello");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));
    let mut c = Client::connect(&d.socket);
    c.send(&ClientMsg::Hello {
        proto: PROTO_VERSION,
        client: ClientKind::Window,
        pid: 1,
    });
    let DaemonMsg::HelloAck { proto, version } = c.recv() else {
        panic!("no ack");
    };
    assert_eq!(proto, PROTO_VERSION);
    assert_eq!(version, "test");
}

#[test]
fn watching_live_serves_rows_identical_to_a_direct_replay() {
    let tmp = Temp::new("live-parity");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));
    let mut c = Client::hello(&d.socket);
    c.watch_live(View::Damage, None);

    // The fixture is fully closed history, so Live resolves to the newest
    // indexed segment; the first useful snapshot arrives once it lazy-loads.
    let mut seen = Vec::new();
    let snap = c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::Snapshot { rows, .. } if !rows.is_empty()),
    );
    let DaemonMsg::Snapshot {
        info,
        rows,
        segment_count,
        source,
        seq,
        ..
    } = snap
    else {
        panic!("unexpected daemon message")
    };

    let lines = fixture_lines();
    let meter = meter_from_lines(lines.iter().map(String::as_str));
    let want = meter.segments().last().unwrap();
    assert_eq!(info.name, want.name);
    assert_eq!(info.name, "Verkath the Hollow");
    assert_eq!(info.duration_ms, 45_000);
    assert_eq!(info.success, Some(false));
    assert!(!info.live, "history is never live");
    assert_eq!(rows, want.rows(View::Damage), "byte-identical rows");
    assert_eq!(segment_count, 5, "4 segments + the visit overall (R10)");
    assert_eq!(source.as_deref(), Some("sample.txt"));
    assert!(seq >= 1);
}

#[test]
fn a_drilled_cursor_carries_the_breakdown() {
    let tmp = Temp::new("drill");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));
    let mut c = Client::hello(&d.socket);

    let lines = fixture_lines();
    let meter = meter_from_lines(lines.iter().map(String::as_str));
    let seg = meter.segments().last().unwrap();
    let top = &seg.rows(View::Damage)[0];

    c.watch_live(View::Damage, Some(&top.key));
    let mut seen = Vec::new();
    let snap = c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::Snapshot { breakdown: Some(b), .. } if !b.by_spell.is_empty()),
    );
    let DaemonMsg::Snapshot { breakdown, .. } = snap else {
        panic!("unexpected daemon message")
    };
    let got = breakdown.unwrap();
    let (want_spell, want_target) = seg.breakdown(&top.key, View::Damage);
    assert_eq!(got.by_spell, want_spell);
    assert_eq!(got.by_target, want_target);
}

#[test]
fn the_list_cursor_serves_the_fixtures_segments_with_stable_ids() {
    let tmp = Temp::new("list");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));
    let mut c = Client::hello(&d.socket);
    c.watch_list();

    let mut seen = Vec::new();
    let list = c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::SegmentList { entries, .. } if entries.len() == 5),
    );
    let DaemonMsg::SegmentList {
        entries, source, ..
    } = list
    else {
        panic!("unexpected daemon message")
    };
    assert_eq!(source.as_deref(), Some("sample.txt"));
    // R10: the fixture takes place inside one raid visit — its Overall row
    // heads the list, before the visit's first member.
    assert_eq!(entries[0].row.kind, SegmentKind::Overall);
    assert_eq!(entries[0].row.name, "Sepulcher of the Ashen Vow");
    assert_eq!(entries[0].row.instance, Some(0));
    assert_eq!(entries[1].row.instance, Some(0), "members carry the visit");
    assert_eq!(entries[2].row.name, "The Ashen Warden");
    assert_eq!(entries[2].row.success, Some(true));
    assert_eq!(entries[2].row.duration_ms, 60_000);
    assert!(
        entries.iter().skip(1).all(|e| !e.row.live),
        "members are history (the never-exited visit's overall stays open)"
    );
    let mut ids: Vec<u64> = entries.iter().map(|e| e.id.0).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "ids are unique");

    // Opening a listed row by id lands on exactly that fight.
    let id = entries[2].id;
    c.watch_id(id, View::Damage);
    let snap = c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::Snapshot { rows, .. } if !rows.is_empty()),
    );
    let DaemonMsg::Snapshot { info, segment, .. } = snap else {
        panic!("unexpected daemon message")
    };
    assert_eq!(segment, SegmentRef::Id(id), "snapshot echoes the cursor");
    assert_eq!(info.name, "The Ashen Warden");
}

#[test]
fn live_appends_reach_watchers_with_monotonic_seq_and_fresh_breakdowns() {
    let tmp = Temp::new("append");
    let log = tmp.join("WoWCombatLog.txt");
    append(&log, &hit(0, 0));
    append(&log, &hit(0, 20));
    let d = start(options(&tmp, SourceSpec::File(log.clone())));

    let mut meter_c = Client::hello(&d.socket);
    meter_c.watch_live(View::Damage, Some("Player-1-A"));
    let mut seen = Vec::new();
    let first = meter_c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::Snapshot { rows, .. } if !rows.is_empty()),
    );
    let DaemonMsg::Snapshot {
        seq: seq1,
        info,
        breakdown,
        ..
    } = first
    else {
        panic!("unexpected daemon message")
    };
    assert!(info.live, "an open trash segment is live");
    let amount_before = breakdown.as_ref().unwrap().by_spell[0].amount;

    // Fresh combat must land after the tailer reaches EOF (CaughtUp); the
    // probes come from Player-1-B, so the drilled breakdown stays untouched.
    wait_caught_up(&d.socket, &log, 21, 40);
    append(&log, &hit(0, 40));

    let second = meter_c.recv_until(&mut seen, |m| {
        matches!(m, DaemonMsg::Snapshot { breakdown: Some(b), .. }
            if b.by_spell.first().is_some_and(|r| r.amount > amount_before))
    });
    let DaemonMsg::Snapshot { seq: seq2, .. } = second else {
        panic!("unexpected daemon message")
    };
    assert!(seq2 > seq1, "seq is monotonic per session");

    // The live drilldown kept updating — the frozen-drilldown regression.
    let all: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    let meter = meter_from_lines(all.iter().map(String::as_str));
    let (want_spell, _) = meter
        .segments()
        .last()
        .unwrap()
        .breakdown("Player-1-A", View::Damage);
    assert_eq!(want_spell[0].amount, amount_before + 900);
}

#[test]
fn a_new_pull_emits_segment_opened_once_and_pulls_the_list_forward() {
    let tmp = Temp::new("opened");
    let log = tmp.join("WoWCombatLog.txt");
    append(&log, &hit(0, 0));
    let d = start(options(&tmp, SourceSpec::File(log.clone())));

    let mut list_c = Client::hello(&d.socket);
    list_c.watch_list();
    let mut seen = Vec::new();
    list_c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::SegmentList { entries, .. } if entries.len() == 1),
    );

    wait_caught_up(&d.socket, &log, 1, 60); // let CaughtUp land, observably
    // A pull well past the trash gap: closes the old segment, opens a new one.
    append(&log, &hit(10, 0));
    append(&log, &hit(10, 5));

    let opened = list_c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::SegmentOpened { .. }));
    let DaemonMsg::SegmentOpened { id } = opened else {
        panic!("unexpected daemon message")
    };
    let list = list_c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::SegmentList { entries, .. } if entries.len() == 2),
    );
    let DaemonMsg::SegmentList { entries, .. } = list else {
        panic!("unexpected daemon message")
    };
    assert_eq!(
        entries.last().unwrap().id,
        id,
        "the opened id is the newest row"
    );
    assert!(entries.last().unwrap().row.live);

    // Same pull continuing must not re-announce.
    append(&log, &hit(10, 10));
    list_c.recv_until(&mut seen, |m| {
        matches!(m, DaemonMsg::SegmentList { entries, .. }
            if entries.last().is_some_and(|e| e.row.duration_ms >= 10_000))
    });
    assert!(
        !seen
            .iter()
            .any(|m| matches!(m, DaemonMsg::SegmentOpened { .. })),
        "SegmentOpened fired again: {seen:#?}"
    );
}

#[test]
fn rotation_retires_old_ids_rather_than_reusing_them() {
    let tmp = Temp::new("rotate");
    let logs = Temp::new("rotate-logs");
    let old = logs.join("WoWCombatLog-01.txt");
    std::fs::copy(FIXTURE, &old).unwrap();
    let d = start(options(&tmp, SourceSpec::Dir(logs.0.clone())));

    let mut c = Client::hello(&d.socket);
    c.watch_list();
    let mut seen = Vec::new();
    let list = c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::SegmentList { entries, .. } if entries.len() == 5),
    );
    let DaemonMsg::SegmentList { entries, .. } = list else {
        panic!("unexpected daemon message")
    };
    let old_id = entries[1].id;
    let old_max = entries.iter().map(|e| e.id.0).max().unwrap();

    // Pin the cursor to a historical fight, then rotate the log out from
    // under it.
    c.watch_id(old_id, View::Damage);
    c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::Snapshot { rows, .. } if !rows.is_empty()),
    );
    let new = logs.join("WoWCombatLog-02.txt");
    append(&new, &hit(0, 0));

    let failed = c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::LoadFailed { .. }));
    let DaemonMsg::LoadFailed { segment, error } = failed else {
        panic!("unexpected daemon message")
    };
    assert_eq!(segment, old_id);
    assert_eq!(error, LoadError::Rotated);

    // The new file's list has fresh ids and the new source name.
    c.watch_list();
    let list = c.recv_until(&mut seen, |m| {
        matches!(m, DaemonMsg::SegmentList { entries, source, .. }
            if !entries.is_empty() && source.as_deref() == Some("WoWCombatLog-02.txt"))
    });
    let DaemonMsg::SegmentList { entries, .. } = list else {
        panic!("unexpected daemon message")
    };
    assert!(
        entries.iter().all(|e| e.id.0 > old_max),
        "ids never reused across rotation: {entries:?} vs {old_max}"
    );
}

#[test]
fn two_clients_on_different_cursors_each_get_only_their_own_feed() {
    let tmp = Temp::new("two");
    let log = tmp.join("WoWCombatLog.txt");
    append(&log, &hit(0, 0));
    let d = start(options(&tmp, SourceSpec::File(log.clone())));

    let mut lister = Client::hello(&d.socket);
    let mut watcher = Client::hello(&d.socket);
    lister.watch_list();
    watcher.watch_live(View::Damage, None);

    let mut lister_seen = Vec::new();
    let mut watcher_seen = Vec::new();
    lister.recv_until(&mut lister_seen, |m| {
        matches!(m, DaemonMsg::SegmentList { .. })
    });
    watcher.recv_until(
        &mut watcher_seen,
        |m| matches!(m, DaemonMsg::Snapshot { rows, .. } if !rows.is_empty()),
    );

    append(&log, &hit(0, 30));
    lister.recv_until(&mut lister_seen, |m| {
        matches!(m, DaemonMsg::SegmentList { entries, .. }
            if entries.last().is_some_and(|e| e.row.duration_ms >= 30_000))
    });
    watcher.recv_until(&mut watcher_seen, |m| {
        matches!(m, DaemonMsg::Snapshot { rows, .. }
            if rows.first().is_some_and(|r| r.amount >= 1_800))
    });

    assert!(
        !lister_seen
            .iter()
            .any(|m| matches!(m, DaemonMsg::Snapshot { .. })),
        "list watcher got a meter snapshot: {lister_seen:#?}"
    );
    assert!(
        !watcher_seen
            .iter()
            .any(|m| matches!(m, DaemonMsg::SegmentList { .. })),
        "meter watcher got a segment list: {watcher_seen:#?}"
    );
}

#[test]
fn shutdown_works_even_before_a_handshake() {
    let tmp = Temp::new("shutdown");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));
    let mut c = Client::connect(&d.socket);
    c.send(&ClientMsg::Shutdown);
    let result = d.done.recv_timeout(DEADLINE).expect("daemon exited");
    assert!(result.is_ok(), "{result:?}");
    assert!(!d.socket.exists(), "socket unlinked on exit");
}

#[test]
fn only_watching_sessions_hold_the_daemon_open() {
    let tmp = Temp::new("idle");
    let mut opts = options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE)));
    opts.linger = false;
    opts.idle_grace = Duration::from_millis(200);
    let d = start(opts);

    // A watching client keeps it alive well past the grace…
    let mut c = Client::hello(&d.socket);
    c.watch_list();
    assert!(
        d.done.recv_timeout(Duration::from_millis(700)).is_err(),
        "daemon idled out despite a watcher"
    );

    // …a GetStatus-only client does not.
    let mut status_only = Client::hello(&d.socket);
    status_only.send(&ClientMsg::GetStatus { req_id: 1 });
    let mut seen = Vec::new();
    status_only.recv_until(&mut seen, |m| matches!(m, DaemonMsg::Status { .. }));

    drop(c);
    let result = d.done.recv_timeout(DEADLINE).expect("daemon idle-exited");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn the_lockfile_lets_exactly_one_daemon_own_the_socket() {
    let tmp = Temp::new("lock");
    let opts_a = options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE)));
    let opts_b = options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE)));
    let d = start(opts_a);
    let _keep = Client::hello(&d.socket);

    let err = run(opts_b).expect_err("second daemon must refuse to start");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);

    // The first daemon is unharmed: its socket still answers.
    let _still = Client::hello(&d.socket);
}

#[test]
fn a_stale_socket_file_is_recovered_not_fatal() {
    let tmp = Temp::new("stale");
    let opts = options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE)));
    // A dead daemon's leftover: bound once, never cleaned up.
    drop(std::os::unix::net::UnixListener::bind(&opts.socket).unwrap());
    assert!(opts.socket.exists());

    let d = start(opts);
    let _c = Client::hello(&d.socket);
}

#[test]
fn an_overlay_session_is_supervised_over_the_wire() {
    let tmp = Temp::new("overlay");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));

    let mut overlay = Client::hello_as(&d.socket, ClientKind::Overlay);
    overlay.watch_live(View::Damage, None);

    let mut poller = Client::hello(&d.socket);
    let mut seen = Vec::new();
    poller.send(&ClientMsg::GetStatus { req_id: 1 });
    let status = poller.recv_until(&mut seen, |m| matches!(m, DaemonMsg::Status { .. }));
    let DaemonMsg::Status { overlay: st, .. } = status else {
        panic!("unexpected daemon message")
    };
    assert_eq!(
        st,
        wowdps_proto::OverlayState::Visible,
        "a connected overlay session reads as visible"
    );

    // The user hides it locally; the supervisor agrees and Status shows it.
    overlay.send(&ClientMsg::VisibilityChanged { visible: false });
    let deadline = Instant::now() + DEADLINE;
    loop {
        assert!(Instant::now() < deadline, "status never flipped to Hidden");
        poller.send(&ClientMsg::GetStatus { req_id: 2 });
        let status = poller.recv_until(&mut seen, |m| {
            matches!(m, DaemonMsg::Status { req_id: 2, .. })
        });
        let DaemonMsg::Status { overlay: st, .. } = status else {
            panic!("unexpected daemon message")
        };
        if st == wowdps_proto::OverlayState::Hidden {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Manual perf gate, a Retail one (which log it reads: core's
/// `tests/real_log/mod.rs`); it prints times, never a name. Run with:
/// `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture`
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_first_snapshot_is_fast_cold_and_faster_warm() {
    let Some(path) = real_log::retail_log() else {
        return;
    };
    let path = PathBuf::from(path);
    let tmp = Temp::new("perf");
    let cache = tmp.join("cache");

    let first_snapshot = |tag: &str, budget_ms: u128| {
        let mut opts = options(&tmp, SourceSpec::File(path.clone()));
        opts.cache_dir = Some(cache.clone());
        let d = start(opts);
        let t = Instant::now();
        let mut c = Client::hello(&d.socket);
        c.watch_live(View::Damage, None);
        let mut seen = Vec::new();
        c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::Snapshot { .. }));
        let ms = t.elapsed().as_millis();
        println!("{tag}: first snapshot in {ms} ms");
        assert!(ms < budget_ms, "{tag} took {ms} ms");
        c.send(&ClientMsg::Shutdown);
        d.done.recv_timeout(DEADLINE).unwrap().unwrap();
    };

    first_snapshot("cold", 1_000);
    first_snapshot("warm", 100);
}

/// Manual perf gate: what a second edition's source costs while no game
/// writes it. Run alone, in release:
/// `cargo test --release -p wowdps-daemon --test ipc -- --ignored idle_edition --nocapture`
/// Each daemon idles at production's tick with a window watching its live
/// segment; the CPU its own threads spend (`/proc/self/task/*/schedstat`,
/// the threads it started) over the idle stretch is printed per source
/// set: Retail's log alone, beside a missing Forever folder, and beside an
/// idle Forever log. The second source must cost under a millisecond of
/// CPU per second.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "a timing measurement: run it alone, in release"]
fn an_idle_edition_costs_close_to_nothing() {
    use std::collections::HashMap;
    use wowdps_model::Edition;
    /// Each thread of this process and the nanoseconds it has run.
    fn tasks() -> HashMap<u64, u64> {
        let mut out = HashMap::new();
        for e in std::fs::read_dir("/proc/self/task")
            .into_iter()
            .flatten()
            .flatten()
        {
            let tid = e.file_name().to_string_lossy().parse::<u64>();
            let ns = std::fs::read_to_string(e.path().join("schedstat"))
                .ok()
                .and_then(|s| s.split_whitespace().next()?.parse::<u64>().ok());
            if let (Ok(tid), Some(ns)) = (tid, ns) {
                out.insert(tid, ns);
            }
        }
        out
    }
    const IDLE: Duration = Duration::from_secs(5);
    let tmp = Temp::new("idle-edition");
    let retail = tmp.join("retail");
    let forever = tmp.join("forever");
    assert!(std::fs::create_dir_all(&retail).is_ok());
    assert!(std::fs::create_dir_all(&forever).is_ok());
    assert!(std::fs::copy(FIXTURE, retail.join("WoWCombatLog-072726_200000.txt")).is_ok());
    let forever_fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");
    assert!(
        std::fs::copy(
            forever_fixture,
            forever.join("WoWCombatLog-110126_200000.txt")
        )
        .is_ok()
    );
    let missing = tmp.join("missing");
    // What is followed beside Retail's log, and how many sources `Status`
    // lists once every listed one holds its log.
    let sets = [
        ("Retail alone", Vec::<(Edition, PathBuf)>::new(), 1),
        (
            "beside a missing Forever folder",
            vec![(Edition::Forever, missing)],
            1,
        ),
        (
            "beside an idle Forever log",
            vec![(Edition::Forever, forever)],
            2,
        ),
    ];
    let mut cost = Vec::new();
    for (what, more, listed) in sets {
        let before = tasks();
        let mut opts = options(&tmp, SourceSpec::Dir(retail.clone()));
        opts.tick = Duration::from_millis(100);
        opts.editions = wowdps_daemon::Followed {
            own: false,
            source: Some(Edition::Retail),
            more,
        };
        let d = start(opts);
        let mut c = Client::hello(&d.socket);
        c.watch_live(View::Damage, None);
        // Caught up: every source listed with its log, then a second more.
        let deadline = Instant::now() + DEADLINE;
        loop {
            assert!(Instant::now() < deadline, "{what}: never listed");
            c.send(&ClientMsg::GetStatus { req_id: 1 });
            let st = c.recv_until(&mut Vec::new(), |m| matches!(m, DaemonMsg::Status { .. }));
            if let DaemonMsg::Status { editions, .. } = st
                && editions.len() == listed
                && editions.iter().all(|e| e.log.is_some())
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_secs(1));
        let ours = |now: &HashMap<u64, u64>| -> u64 {
            now.iter()
                .filter(|(tid, _)| !before.contains_key(tid))
                .map(|(_, ns)| *ns)
                .sum()
        };
        let (t0, at) = (ours(&tasks()), Instant::now());
        thread::sleep(IDLE);
        let (t1, secs) = (ours(&tasks()), at.elapsed().as_secs_f64());
        let per_s = (t1.saturating_sub(t0)) as f64 / 1e6 / secs;
        println!("{what}: {per_s:.3} ms of CPU per second idle");
        cost.push(per_s);
        c.send(&ClientMsg::Shutdown);
        let _ = d.done.recv_timeout(DEADLINE);
    }
    for (i, c) in cost.iter().enumerate().skip(1) {
        assert!(
            c - cost[0] < 1.0,
            "set {i} costs {c:.3} ms/s against {:.3}",
            cost[0]
        );
    }
}

/// v49 end to end: a daemon following two editions' folders, one source
/// and one engine each. `Status` lists both, the live one marked; a session
/// pinned to Retail is answered Retail's log whichever is live; a
/// following one is answered the live edition's — the log showing the
/// newest line — and is moved, its frames naming the other edition, when
/// the other log shows a newer line and the last move's hold is over.
#[test]
fn a_daemon_follows_one_source_per_edition_and_moves_its_followers() {
    use wowdps_model::Edition;
    use wowdps_proto::EditionRef;
    let tmp = Temp::new("editions");
    let retail = tmp.join("retail");
    let forever = tmp.join("forever");
    assert!(std::fs::create_dir_all(&retail).is_ok());
    assert!(std::fs::create_dir_all(&forever).is_ok());
    let retail_log = retail.join("WoWCombatLog-072726_200000.txt");
    assert!(std::fs::copy(FIXTURE, &retail_log).is_ok());
    let forever_fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");
    assert!(
        std::fs::copy(
            forever_fixture,
            forever.join("WoWCombatLog-110126_200000.txt")
        )
        .is_ok()
    );
    // Retail's log was written an hour ago; Forever's just now.
    let hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    let set = std::fs::File::options()
        .write(true)
        .open(&retail_log)
        .and_then(|f| f.set_modified(hour_ago));
    assert!(set.is_ok(), "{set:?}");

    let mut opts = options(&tmp, SourceSpec::Dir(retail.clone()));
    opts.editions = wowdps_daemon::Followed {
        own: false,
        source: Some(Edition::Retail),
        more: vec![(Edition::Forever, forever)],
    };
    let d = start(opts);
    let edition_of = |m: &DaemonMsg| match m {
        DaemonMsg::SegmentList {
            entries,
            edition,
            source,
            ..
        } if !entries.is_empty() => Some((*edition, source.clone())),
        _ => None,
    };

    let mut follower = Client::hello(&d.socket);
    follower.watch_list();
    let mut seen = Vec::new();
    follower.recv_until(&mut seen, |m| {
        edition_of(m).is_some_and(|(e, _)| e == Some(Edition::Forever))
    });

    let mut pinned = Client::hello(&d.socket);
    pinned.send(&ClientMsg::Watch {
        cursor: Cursor::List,
        edition: EditionRef::Pin(Edition::Retail),
    });
    let list = pinned.recv_until(&mut Vec::new(), |m| edition_of(m).is_some());
    assert_eq!(
        edition_of(&list),
        Some((
            Some(Edition::Retail),
            Some("WoWCombatLog-072726_200000.txt".to_string())
        ))
    );

    let mut status = Client::hello(&d.socket);
    status.send(&ClientMsg::GetStatus { req_id: 1 });
    match status.recv_until(&mut Vec::new(), |m| matches!(m, DaemonMsg::Status { .. })) {
        DaemonMsg::Status { editions, .. } => {
            let got: Vec<_> = editions.iter().map(|e| (e.edition, e.live)).collect();
            assert_eq!(
                got,
                [
                    (Some(Edition::Retail), false),
                    (Some(Edition::Forever), true)
                ]
            );
        }
        other => panic!("{other:?}"),
    }

    // Retail's log is written now, with lines later than Forever's newest
    // (its fixture ends on 11/1/2026): Retail's log shows the newest line,
    // and the follower is moved once the start-up move to Forever has held
    // the live edition its `HANDOFF_QUIET`.
    let mut seen = Vec::new();
    let deadline = Instant::now() + wowdps_daemon::sources::HANDOFF_QUIET + DEADLINE;
    let mut sec = 0;
    loop {
        assert!(
            Instant::now() < deadline,
            "the follower was never moved: {seen:#?}"
        );
        append(
            &retail_log,
            &format!(
                "11/2/2026 10:{:02}:{:02}.000-8  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n",
                30 + sec / 60,
                sec % 60
            ),
        );
        sec += 1;
        thread::sleep(Duration::from_millis(50));
        follower.send(&ClientMsg::GetStatus { req_id: 2 });
        let status = follower.recv_until(&mut seen, |m| matches!(m, DaemonMsg::Status { .. }));
        if let DaemonMsg::Status { editions, .. } = status
            && editions.first().is_some_and(|e| e.live)
        {
            break;
        }
    }
    // The move: a list naming Retail's log, sent to the follower.
    let named_retail = |m: &DaemonMsg| {
        edition_of(m).is_some_and(|(e, s)| {
            e == Some(Edition::Retail) && s.as_deref() == Some("WoWCombatLog-072726_200000.txt")
        })
    };
    if !seen.iter().any(named_retail) {
        follower.recv_until(&mut seen, named_retail);
    }
    follower.send(&ClientMsg::Shutdown);
    let _ = d.done.recv_timeout(DEADLINE);
}
