//! `DaemonClient` against a scripted peer on a socketpair: handshake, then
//! the coalescing guarantee — a stalled reader catches up to the newest
//! snapshot per cursor, never a backlog.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use wowdps_model::{SegmentInfo, SegmentKind, View};
use wowdps_proto::wire;
use wowdps_proto::{ClientKind, ClientMsg, DaemonClient, DaemonMsg, PROTO_VERSION, SegmentRef};

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
