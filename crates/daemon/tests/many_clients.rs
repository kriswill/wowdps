//! Many-client integration suite: one real daemon on a temp socket, a dozen
//! concurrent protocol clients with mixed cursors, and churn. Scaffolding is
//! adapted from `ipc.rs` (kept separate so both suites can evolve alone).

use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wowdps_core::model::{SegmentId, View};
use wowdps_core::tail::SourceSpec;
use wowdps_daemon::{DaemonOptions, run};
use wowdps_proto::{ClientKind, ClientMsg, Cursor, DaemonMsg, PROTO_VERSION, SegmentRef, wire};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const DEADLINE: Duration = Duration::from_secs(10);

// ---- scaffolding (adapted from ipc.rs) ---------------------------------------

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("wowdps-many-{tag}-{}", std::process::id()));
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
        let mut c = Self::connect(socket);
        c.send(&ClientMsg::Hello {
            proto: PROTO_VERSION,
            client: ClientKind::Tui,
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

    fn watch(&mut self, cursor: Cursor) {
        self.send(&ClientMsg::Watch(cursor));
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

/// The per-session seq stamped on a pushed snapshot/list, if this message
/// carries one (control messages like SegmentOpened do not).
fn seq_of(msg: &DaemonMsg) -> Option<u64> {
    match msg {
        DaemonMsg::Snapshot { seq, .. }
        | DaemonMsg::SegmentList { seq, .. }
        | DaemonMsg::CompareSnapshot { seq, .. } => Some(*seq),
        _ => None,
    }
}

// ---- tests ------------------------------------------------------------------

/// A dozen concurrent clients on a mix of cursors: list watchers, per-id
/// meter watchers across several views, and Live watchers. Each must receive
/// exactly its own feed — every Snapshot echoes the cursor that asked for it
/// (the SegmentList broadcast is the one contractual exception) — with a
/// per-session strictly monotonic seq.
#[test]
fn a_dozen_mixed_cursors_each_get_only_their_own_feed() {
    let tmp = Temp::new("mixed");
    let d = start(options(&tmp, SourceSpec::File(PathBuf::from(FIXTURE))));

    // Bootstrap: one client resolves the fixture's stable ids first, so the
    // worker clients can pin real segments. sample.txt lists 5 entries: the
    // visit's Σ Overall plus 4 members (see ipc.rs's list test).
    let ids: Vec<SegmentId> = {
        let mut c = Client::hello(&d.socket);
        c.watch(Cursor::List);
        let mut seen = Vec::new();
        let list = c.recv_until(
            &mut seen,
            |m| matches!(m, DaemonMsg::SegmentList { entries, .. } if entries.len() == 5),
        );
        let DaemonMsg::SegmentList { entries, .. } = list else {
            panic!("unexpected daemon message")
        };
        entries.iter().map(|e| e.id).collect()
    };

    let views = [
        View::Damage,
        View::Healing,
        View::Deaths,
        View::Interrupts,
        View::Taken,
    ];
    let socket = d.socket.clone();
    let mut workers = Vec::new();
    for n in 0..12usize {
        let socket = socket.clone();
        let ids = ids.clone();
        workers.push(thread::spawn(move || {
            let mut c = Client::hello(&socket);
            // 0,3,6,9 → list; 1,4,7,10 → a pinned id (skipping the Σ row so
            // views vary meaningfully); 2,5,8,11 → Live.
            let cursor = match n % 3 {
                0 => Cursor::List,
                1 => Cursor::Segment {
                    segment: SegmentRef::Id(ids[1 + n % 4]),
                    view: views[n % views.len()],
                    top_n: None,
                    drill: None,
                    spell: None,
                },
                _ => Cursor::Segment {
                    segment: SegmentRef::Live,
                    view: views[n % views.len()],
                    top_n: None,
                    drill: None,
                    spell: None,
                },
            };
            c.watch(cursor.clone());

            // Collect until the cursor's real answer arrives (a lazily
            // loaded segment sends a "loading…" placeholder first; both
            // count as our feed).
            let mut seen = Vec::new();
            let last = c.recv_until(&mut seen, |m| match (&cursor, m) {
                (Cursor::List, DaemonMsg::SegmentList { entries, .. }) => entries.len() == 5,
                (Cursor::Segment { .. }, DaemonMsg::Snapshot { id, .. }) => id.is_some(),
                _ => false,
            });
            seen.push(last);

            // Own feed only: every Snapshot echoes this client's cursor.
            // SegmentList may reach anyone (the broadcast), but nothing else
            // foreign may.
            for m in &seen {
                match (&cursor, m) {
                    (_, DaemonMsg::SegmentList { .. }) => {}
                    (
                        Cursor::Segment { segment, view, .. },
                        DaemonMsg::Snapshot {
                            segment: got_seg,
                            view: got_view,
                            ..
                        },
                    ) => {
                        assert_eq!(got_seg, segment, "client {n}: foreign segment");
                        assert_eq!(got_view, view, "client {n}: foreign view");
                    }
                    (Cursor::List, DaemonMsg::Snapshot { .. }) => {
                        panic!("client {n}: list watcher got a meter snapshot")
                    }
                    (_, other) => panic!("client {n}: unexpected message {other:?}"),
                }
            }

            // Per-session seq is strictly monotonic across everything
            // stamped (snapshots and list broadcasts share one counter).
            let seqs: Vec<u64> = seen.iter().filter_map(seq_of).collect();
            assert!(!seqs.is_empty(), "client {n} never got a stamped message");
            assert!(
                seqs.windows(2).all(|w| w[1] > w[0]),
                "client {n}: seq not strictly monotonic: {seqs:?}"
            );
        }));
    }
    for (n, w) in workers.into_iter().enumerate() {
        assert!(w.join().is_ok(), "client {n} panicked");
    }
}

/// Clients churning — connecting, watching, and vanishing at every stage of
/// the handshake — must neither wedge the daemon nor leak sessions: a steady
/// watcher keeps receiving live updates throughout, and Status's client
/// count settles back down to exactly the survivors.
#[test]
fn churn_does_not_wedge_the_daemon_or_leak_sessions() {
    let tmp = Temp::new("churn");
    let log = tmp.join("WoWCombatLog.txt");
    append(&log, &hit(0, 0));
    let d = start(options(&tmp, SourceSpec::File(log.clone())));

    // The steady watcher: alive across all churn.
    let mut steady = Client::hello(&d.socket);
    steady.watch(Cursor::Segment {
        segment: SegmentRef::Live,
        view: View::Damage,
        top_n: None,
        drill: None,
        spell: None,
    });
    let mut steady_seen = Vec::new();
    steady.recv_until(
        &mut steady_seen,
        |m| matches!(m, DaemonMsg::Snapshot { rows, .. } if !rows.is_empty()),
    );

    // Churn: 24 short-lived clients dropped at three different stages —
    // mid-handshake, post-hello, and mid-watch (after one answer).
    for i in 0..24usize {
        match i % 3 {
            0 => {
                // Connected but never spoke: the reader thread sees EOF.
                let _c = Client::connect(&d.socket);
            }
            1 => {
                // Handshook, never watched.
                let _c = Client::hello(&d.socket);
            }
            _ => {
                // Watched and got an answer, then vanished.
                let mut c = Client::hello(&d.socket);
                c.watch(Cursor::List);
                let mut seen = Vec::new();
                c.recv_until(&mut seen, |m| matches!(m, DaemonMsg::SegmentList { .. }));
            }
        }
    }

    // The daemon still serves the survivor: an appended line must show up
    // in its snapshot (backlog or fresh — either proves the pipeline moves).
    append(&log, &hit(0, 30));
    steady.recv_until(&mut steady_seen, |m| {
        matches!(m, DaemonMsg::Snapshot { rows, .. }
            if rows.first().is_some_and(|r| r.amount >= 1_800))
    });

    // No leaked sessions: a status poller and the steady watcher are the
    // only clients left once the reaper catches up. Deadline-poll — session
    // teardown is asynchronous (reader threads notice EOF on their own).
    let mut poller = Client::hello(&d.socket);
    let deadline = Instant::now() + DEADLINE;
    let mut req_id = 0u32;
    loop {
        req_id += 1;
        poller.send(&ClientMsg::GetStatus { req_id });
        let mut seen = Vec::new();
        let want = req_id;
        let status = poller.recv_until(
            &mut seen,
            move |m| matches!(m, DaemonMsg::Status { req_id, .. } if *req_id == want),
        );
        let DaemonMsg::Status { clients, .. } = status else {
            panic!("unexpected daemon message")
        };
        if clients == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "sessions leaked: still {clients} clients"
        );
        thread::sleep(Duration::from_millis(20));
    }

    // And it still shuts down cleanly — the final wedge check.
    poller.send(&ClientMsg::Shutdown);
    let result = d.done.recv_timeout(DEADLINE).expect("daemon exited");
    assert!(result.is_ok(), "{result:?}");
}
