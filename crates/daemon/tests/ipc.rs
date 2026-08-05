//! Integration suite: real daemons on temp sockets, real fixture data, real
//! protocol frames. Every test drives `wowdps_daemon::run` the way a client
//! binary would.

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
        std::fs::create_dir_all(&p).unwrap();
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
    }
}

fn start(opts: DaemonOptions) -> Daemon {
    let socket = opts.socket.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(run(opts));
    });
    let deadline = Instant::now() + DEADLINE;
    while !socket.exists() {
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
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                Err(e) => panic!("connect {socket:?}: {e}"),
            }
        };
        stream.set_read_timeout(Some(DEADLINE)).unwrap();
        Client { stream }
    }

    fn send(&mut self, msg: &ClientMsg) {
        self.stream.write_all(&msg.encode()).unwrap();
    }

    fn recv(&mut self) -> DaemonMsg {
        let (tag, body) = wire::read_frame(&mut self.stream).expect("daemon frame");
        DaemonMsg::decode(tag, &body).expect("valid daemon message")
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
        let DaemonMsg::HelloAck { proto, .. } = ack else {
            panic!("expected HelloAck, got {ack:?}");
        };
        assert_eq!(proto, PROTO_VERSION);
        c
    }

    fn watch_live(&mut self, view: View, drill: Option<&str>) {
        self.send(&ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Live,
            view,
            top_n: None,
            drill: drill.map(str::to_string),
        }));
    }

    fn watch_id(&mut self, id: SegmentId, view: View) {
        self.send(&ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Id(id),
            view,
            top_n: None,
            drill: None,
        }));
    }

    fn watch_list(&mut self) {
        self.send(&ClientMsg::Watch(Cursor::List));
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
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn fixture_lines() -> Vec<String> {
    std::fs::read_to_string(FIXTURE)
        .unwrap()
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
        unreachable!()
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
        unreachable!()
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
        unreachable!()
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
        unreachable!()
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
        unreachable!()
    };
    assert!(info.live, "an open trash segment is live");
    let amount_before = breakdown.as_ref().unwrap().by_spell[0].amount;

    // Give the tailer time to reach EOF (CaughtUp) before fresh combat.
    thread::sleep(Duration::from_millis(600));
    append(&log, &hit(0, 40));

    let second = meter_c.recv_until(&mut seen, |m| {
        matches!(m, DaemonMsg::Snapshot { breakdown: Some(b), .. }
            if b.by_spell.first().is_some_and(|r| r.amount > amount_before))
    });
    let DaemonMsg::Snapshot { seq: seq2, .. } = second else {
        unreachable!()
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

    thread::sleep(Duration::from_millis(600)); // let CaughtUp land
    // A pull well past the trash gap: closes the old segment, opens a new one.
    append(&log, &hit(10, 0));
    append(&log, &hit(10, 5));

    let opened = list_c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::SegmentOpened { .. }));
    let DaemonMsg::SegmentOpened { id } = opened else {
        unreachable!()
    };
    let list = list_c.recv_until(
        &mut seen,
        |m| matches!(m, DaemonMsg::SegmentList { entries, .. } if entries.len() == 2),
    );
    let DaemonMsg::SegmentList { entries, .. } = list else {
        unreachable!()
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
        unreachable!()
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
        unreachable!()
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
        unreachable!()
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
    thread::sleep(Duration::from_millis(700));
    assert!(
        d.done.try_recv().is_err(),
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
        unreachable!()
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
            unreachable!()
        };
        if st == wowdps_proto::OverlayState::Hidden {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Manual perf gate. Run with:
/// `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon -- --ignored real_log --nocapture`
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_first_snapshot_is_fast_cold_and_faster_warm() {
    let path = PathBuf::from(std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG"));
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
