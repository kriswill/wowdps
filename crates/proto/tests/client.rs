//! `DaemonClient` against a scripted peer on a socketpair: handshake, then
//! the coalescing guarantee — a stalled reader catches up to the newest
//! snapshot per cursor, never a backlog.

use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wowdps_model::{SegmentInfo, SegmentKind, View};
use wowdps_proto::msg::Cursor;
use wowdps_proto::wire;
use wowdps_proto::{
    ClientKind, ClientMsg, DaemonClient, DaemonMsg, PROTO_VERSION, SegmentRef, SourceArg,
    ensure_daemon, socket_path,
};

fn snapshot(seq: u64, view: View, status: &str) -> DaemonMsg {
    DaemonMsg::Snapshot {
        seq,
        segment: SegmentRef::Live,
        id: None,
        view,
        info: SegmentInfo {
            kind: SegmentKind::Trash,
            name: "x".to_string(),
            start_ms: 0,
            duration_ms: 0,
            success: None,
            live: true,
            instance: None,
            pars_ms: None,
            arena: false,
        },
        rows: vec![],
        total_rows: 0,
        breakdown: None,
        segment_count: 1,
        source: None,
        status: Some(status.to_string()),
    }
}

fn list(seq: u64) -> DaemonMsg {
    DaemonMsg::SegmentList {
        seq,
        entries: vec![],
        source: None,
        active: false,
    }
}

#[test]
fn a_stalled_client_receives_the_newest_snapshots_not_a_backlog() {
    let (ours, theirs) = UnixStream::pair().unwrap();

    let server = std::thread::spawn(move || {
        let mut s = theirs;
        // Expect the Hello, answer it.
        let (tag, body) = wire::read_frame(&mut s).unwrap();
        let hello = ClientMsg::decode(tag, &body).unwrap();
        assert!(matches!(
            hello,
            ClientMsg::Hello { proto, client: ClientKind::Window, .. } if proto == PROTO_VERSION
        ));
        s.write_all(
            &DaemonMsg::HelloAck {
                proto: PROTO_VERSION,
                version: "test".to_string(),
            }
            .encode(),
        )
        .unwrap();

        // A burst the client is too stalled to drain per-message: three
        // ticks of one cursor, one of another view, two lists, one control.
        for msg in [
            snapshot(1, View::Damage, "stale"),
            snapshot(2, View::Damage, "stale"),
            list(5),
            snapshot(3, View::Damage, "fresh"),
            snapshot(1, View::Healing, "other-cursor"),
            list(6),
            DaemonMsg::SegmentOpened {
                id: wowdps_model::SegmentId(42),
            },
        ] {
            s.write_all(&msg.encode()).unwrap();
        }
        // Closing flushes EOF so the client knows the stream is done.
    });

    let mut client = DaemonClient::over(ours, ClientKind::Window).unwrap();
    server.join().unwrap();

    // Wait until the reader thread has drained the whole burst (EOF seen).
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.is_dead() {
        assert!(Instant::now() < deadline, "reader never hit EOF");
        std::thread::sleep(Duration::from_millis(5));
    }

    let msgs = client.poll();
    let snaps: Vec<&DaemonMsg> = msgs
        .iter()
        .filter(|m| matches!(m, DaemonMsg::Snapshot { .. }))
        .collect();
    assert_eq!(snaps.len(), 2, "one per (segment, view): {msgs:#?}");
    assert!(
        snaps.iter().any(
            |m| matches!(m, DaemonMsg::Snapshot { seq: 3, status: Some(s), .. } if s == "fresh")
        ),
        "the newest Damage snapshot survives"
    );
    assert!(
        !msgs
            .iter()
            .any(|m| matches!(m, DaemonMsg::Snapshot { status: Some(s), .. } if s == "stale")),
        "stale snapshots were coalesced away"
    );

    let lists: Vec<&DaemonMsg> = msgs
        .iter()
        .filter(|m| matches!(m, DaemonMsg::SegmentList { .. }))
        .collect();
    assert_eq!(lists.len(), 1);
    assert!(matches!(lists[0], DaemonMsg::SegmentList { seq: 6, .. }));

    assert!(
        msgs.iter()
            .any(|m| matches!(m, DaemonMsg::SegmentOpened { id } if id.0 == 42)),
        "control messages are never dropped"
    );

    // Everything drained: a second poll is empty.
    assert!(client.poll().is_empty());
}

fn compare(seq: u64, status: &str) -> DaemonMsg {
    DaemonMsg::CompareSnapshot {
        seq,
        segment: SegmentRef::Live,
        id: None,
        info: SegmentInfo {
            kind: SegmentKind::Trash,
            name: "x".to_string(),
            start_ms: 0,
            duration_ms: 0,
            success: None,
            live: true,
            instance: None,
            pars_ms: None,
            arena: false,
        },
        a: Box::default(),
        b: Box::default(),
        source: None,
        status: Some(status.to_string()),
    }
}

/// Handshake a scripted peer and hand back the served side. Result-shaped
/// because a free helper in an integration test is not "in tests" to
/// clippy's expect ban — callers unwrap inside their `#[test]` fn.
fn served_client(kind: ClientKind) -> std::io::Result<(DaemonClient, UnixStream)> {
    let (ours, theirs) = UnixStream::pair()?;
    let server = std::thread::spawn(move || -> std::io::Result<UnixStream> {
        let mut s = theirs;
        let (tag, body) = wire::read_frame(&mut s)?;
        if !matches!(ClientMsg::decode(tag, &body), Ok(ClientMsg::Hello { .. })) {
            return Err(std::io::Error::other("first frame was not a Hello"));
        }
        s.write_all(
            &DaemonMsg::HelloAck {
                proto: PROTO_VERSION,
                version: "test".to_string(),
            }
            .encode(),
        )?;
        Ok(s)
    });
    let client = DaemonClient::over(ours, kind)?;
    let stream = server
        .join()
        .map_err(|_| std::io::Error::other("server thread panicked"))??;
    Ok((client, stream))
}

fn wait_dead(client: &mut DaemonClient) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.is_dead() {
        assert!(Instant::now() < deadline, "client never noticed the hangup");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_bad_handshake_reply_is_an_error_not_a_hang() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        let mut s = theirs;
        let _ = wire::read_frame(&mut s);
        let _ = s.write_all(&DaemonMsg::Fatal("bad handshake".to_string()).encode());
    });
    let err = match DaemonClient::over(ours, ClientKind::Tui) {
        Ok(_) => panic!("handshake should have failed"),
        Err(e) => e,
    };
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

/// A HelloAck from the future (or the past) is a version skew, not a
/// connection — the socket name embeds `PROTO_VERSION` precisely so this
/// can only happen to a misbuilt daemon, and it must fail loudly.
#[test]
fn a_wrong_proto_hello_ack_is_rejected() {
    let (ours, theirs) = UnixStream::pair().expect("socketpair");
    std::thread::spawn(move || {
        let mut s = theirs;
        let _ = wire::read_frame(&mut s);
        let _ = s.write_all(
            &DaemonMsg::HelloAck {
                proto: PROTO_VERSION + 1,
                version: "future".to_string(),
            }
            .encode(),
        );
    });
    let err = match DaemonClient::over(ours, ClientKind::Tui) {
        Ok(_) => panic!("a wrong-proto ack should have failed the handshake"),
        Err(e) => e,
    };
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

/// An undecodable frame after the handshake kills the reader thread —
/// quietly, marking the client dead — and never takes the process down.
/// Messages that arrived before the poison are still delivered.
#[test]
fn an_undecodable_frame_kills_the_reader_and_marks_the_client_dead() {
    let (mut client, mut server) = served_client(ClientKind::Window).expect("served client");
    server
        .write_all(&list(7).encode())
        .expect("good frame first");
    // Tag 0xEE names no DaemonMsg; the reader must treat it as corruption.
    server
        .write_all(&wire::frame(0xEE, &[1, 2, 3]))
        .expect("poison frame");
    wait_dead(&mut client);
    assert!(
        client
            .poll()
            .iter()
            .any(|m| matches!(m, DaemonMsg::SegmentList { seq: 7, .. })),
        "the message before the poison frame still arrives"
    );
}

/// Writes after the daemon hangs up mark the client dead instead of
/// erroring out of the render loop; `poll` on the dead client keeps
/// answering (emptily) rather than blocking or panicking.
#[test]
fn sending_after_the_daemon_hangs_up_marks_the_client_dead() {
    let (mut client, server) = served_client(ClientKind::Tui).expect("served client");
    drop(server);
    wait_dead(&mut client);
    client.send(&ClientMsg::Watch(Cursor::List));
    assert!(client.is_dead(), "a failed write is a death sentence");
    assert!(client.poll().is_empty());
}

/// R12: comparison snapshots coalesce like meter snapshots — a stalled
/// client sees only the newest one, and meter snapshots are not caught
/// in the same net.
#[test]
fn compare_snapshots_coalesce_to_the_newest() {
    let (mut client, mut server) = served_client(ClientKind::Window).expect("served client");
    for msg in [
        compare(1, "stale"),
        snapshot(1, View::Damage, "meter"),
        compare(2, "fresh"),
    ] {
        server.write_all(&msg.encode()).expect("burst");
    }
    drop(server);
    wait_dead(&mut client);

    let msgs = client.poll();
    let compares: Vec<&DaemonMsg> = msgs
        .iter()
        .filter(|m| matches!(m, DaemonMsg::CompareSnapshot { .. }))
        .collect();
    assert_eq!(compares.len(), 1, "newest comparison only: {msgs:#?}");
    assert!(matches!(
        compares.as_slice(),
        [DaemonMsg::CompareSnapshot { seq: 2, status: Some(s), .. }] if s == "fresh"
    ));
    assert!(
        msgs.iter().any(|m| matches!(m, DaemonMsg::Snapshot { .. })),
        "the meter snapshot rides along untouched"
    );
}

/// A client served over a bare stream (tests, `--status`) has no daemon
/// binary to respawn; reconnect must refuse rather than guess.
#[test]
fn reconnecting_is_refused_without_a_daemon_binary() {
    let (mut client, server) = served_client(ClientKind::Tui).expect("served client");
    drop(server);
    wait_dead(&mut client);
    assert!(!client.reconnect_if_dead());
    assert!(client.is_dead());
}

/// The whole spawn/reconnect surface, serialized in one test because
/// `socket_path()` reads process-global env: a squatted socket dir is
/// refused; a missing daemon binary fails the spawn; a spawned daemon that
/// never listens times out; a listener that appears late is found by the
/// retry loop; and a dead watching client respawns, re-handshakes, and
/// re-declares its cursor unprompted.
#[test]
fn spawning_reconnecting_and_squatted_dirs() {
    let tmp = std::env::temp_dir().join(format!("wowdps-client-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).expect("temp dir");
    // Env is process-global: this integration binary's only env-touching
    // test, mirroring the unit-test precedent in client.rs.
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", &tmp) };
    let sh = std::path::Path::new("/bin/sh");

    // A file squatting the socket-dir name is refused, not used.
    std::fs::write(tmp.join("wowdps"), b"squatter").expect("squat");
    assert!(
        ensure_daemon(sh, None).is_err(),
        "a squatted dir must fail the connect"
    );
    std::fs::remove_file(tmp.join("wowdps")).expect("unsquat");

    // A daemon binary that does not exist fails at spawn, quickly.
    assert!(ensure_daemon(std::path::Path::new("/nonexistent/wowdps-daemon"), None).is_err());

    // A binary that spawns fine but never listens: the retry loop gives up.
    // (/bin/sh exits immediately on the unknown --daemon flag.)
    let err = match ensure_daemon(sh, Some(&SourceArg::File(PathBuf::from("/dev/null")))) {
        Ok(_) => panic!("nothing is listening; connect cannot succeed"),
        Err(e) => e,
    };
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    // A listener that appears mid-retry is found without a respawn race.
    let path = socket_path();
    let late = {
        let path = path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            UnixListener::bind(&path).expect("late bind")
        })
    };
    assert!(
        ensure_daemon(sh, Some(&SourceArg::Logs(PathBuf::from("/tmp")))).is_ok(),
        "the retry loop finds a late listener"
    );
    drop(late.join().expect("late listener thread"));
    std::fs::remove_file(&path).expect("clear late socket");

    // A fake daemon that acks every connection, reports what it reads, and
    // keeps a clone of each accepted conn so the test can hang up on demand.
    let listener = UnixListener::bind(&path).expect("bind");
    let (seen_tx, seen_rx) = std::sync::mpsc::channel::<ClientMsg>();
    let conns: Arc<Mutex<Vec<UnixStream>>> = Arc::new(Mutex::new(Vec::new()));
    let conns_for_daemon = Arc::clone(&conns);
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { break };
            if let (Ok(clone), Ok(mut held)) = (s.try_clone(), conns_for_daemon.lock()) {
                held.push(clone);
            }
            let tx = seen_tx.clone();
            std::thread::spawn(move || {
                while let Ok((tag, body)) = wire::read_frame(&mut s) {
                    let Ok(msg) = ClientMsg::decode(tag, &body) else {
                        break;
                    };
                    if matches!(msg, ClientMsg::Hello { .. }) {
                        let ack = DaemonMsg::HelloAck {
                            proto: PROTO_VERSION,
                            version: "fake".to_string(),
                        };
                        if s.write_all(&ack.encode()).is_err() {
                            break;
                        }
                    }
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            });
        }
    });
    let expect_watch = |what: &str| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match seen_rx.recv_timeout(Duration::from_secs(1)) {
                Ok(ClientMsg::Watch(c)) => return c,
                Ok(_) => {}
                Err(_) => assert!(Instant::now() < deadline, "{what} never arrived"),
            }
        }
    };

    // connect() takes the fast path: a listener exists, nothing is spawned.
    let mut client = DaemonClient::connect(sh, None, ClientKind::Tui).expect("connect");
    let cursor = Cursor::Segment {
        segment: SegmentRef::Live,
        view: View::Damage,
        top_n: None,
        drill: None,
    };
    client.watch(cursor.clone());
    assert_eq!(expect_watch("the watch"), cursor);

    // The daemon hangs up mid-watch; the listener itself stays alive, so a
    // reconnect must succeed and re-declare the cursor without being asked.
    if let Ok(mut held) = conns.lock() {
        for c in held.drain(..) {
            let _ = c.shutdown(std::net::Shutdown::Both);
        }
    }
    wait_dead(&mut client);
    assert!(
        client.reconnect_if_dead(),
        "listener is alive; must succeed"
    );
    assert!(!client.is_dead());
    assert_eq!(
        expect_watch("the re-declared watch"),
        cursor,
        "a reconnect re-declares the last cursor unprompted"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
