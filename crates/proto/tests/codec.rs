//! Codec suite: every variant roundtrips, every truncation errors (never
//! panics), garbage is rejected, and golden bytes force a conscious
//! `PROTO_VERSION` bump whenever an encoded shape changes.

use wowdps_model::{Class, ListRow, Row, SegmentId, SegmentInfo, SegmentKind, Spec, View};
use wowdps_proto::wire::{self, DecodeError};
use wowdps_proto::{
    Breakdown, ClientKind, ClientMsg, Cursor, DaemonMsg, ListEntry, LoadError, OverlayState,
    PROTO_VERSION, SegmentRef,
};

fn row(key: &str, class: Option<Class>) -> Row {
    Row {
        key: key.to_string(),
        label: format!("«{key}»"), // non-ASCII on purpose
        amount: u64::MAX,
        extra: 7,
        count: 1234,
        crits: u64::MAX,
        per_sec: 123456.789,
        pct: 99.25,
        class,
        // Exercise both arms of the spec field: classed rows carry one,
        // classless rows don't. (The wire doesn't cross-check spec vs class.)
        spec: class.map(|c| match c {
            Class::Mage => Spec::FrostMage,
            _ => Spec::Devastation,
        }),
        // Likewise both arms of the recap fields: classed rows carry HP and
        // read as gains, classless rows don't.
        hp: class.map(|_| (123_456, u64::MAX)),
        gain: class.is_some(),
    }
}

fn info() -> SegmentInfo {
    SegmentInfo {
        kind: SegmentKind::Encounter,
        name: "Verkath the Hollow".to_string(),
        start_ms: -62_135_596_800_000, // i64 edge-ish: far-past timestamp
        duration_ms: 45_000,
        success: Some(false),
        live: true,
        instance: Some(7),
        pars_ms: Some((1_680_000, 1_344_000, 1_008_000)),
    }
}

fn list_row(live: bool) -> ListRow {
    ListRow {
        kind: SegmentKind::Overall,
        name: "Häxenmeister +3".to_string(),
        start_ms: 1_722_000_000_123,
        success: None,
        duration_ms: 61_500,
        live,
        instance: Some(0),
        pars_ms: Some((2_040_000, 1_632_000, 1_224_000)),
    }
}

/// Every ClientMsg variant, edge values included.
fn client_msgs() -> Vec<ClientMsg> {
    vec![
        ClientMsg::Hello {
            proto: PROTO_VERSION,
            client: ClientKind::Overlay,
            pid: u32::MAX,
        },
        ClientMsg::Watch(Cursor::List),
        ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Live,
            view: View::Damage,
            top_n: None,
            drill: None,
        }),
        ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Id(SegmentId(u64::MAX)),
            view: View::Deaths,
            top_n: Some(0),
            drill: Some("Player-1301-0AB7C3D2".to_string()),
        }),
        ClientMsg::GetStatus { req_id: 42 },
        ClientMsg::VisibilityChanged { visible: false },
        ClientMsg::Shutdown,
    ]
}

/// Every DaemonMsg variant, edge values included.
fn daemon_msgs() -> Vec<DaemonMsg> {
    vec![
        DaemonMsg::HelloAck {
            proto: PROTO_VERSION,
            version: "0.1.0".to_string(),
        },
        DaemonMsg::Snapshot {
            seq: u64::MAX,
            segment: SegmentRef::Id(SegmentId(3)),
            id: Some(SegmentId(3)),
            view: View::Healing,
            info: info(),
            rows: vec![row("Player-1-A", Some(Class::Evoker)), row("Pet-x", None)],
            total_rows: 40,
            breakdown: Some(Breakdown {
                by_spell: vec![row("Frostbolt", Some(Class::Mage))],
                by_target: vec![],
            }),
            segment_count: 12,
            source: Some("WoWCombatLog-080226_190155.txt".to_string()),
            status: None,
        },
        DaemonMsg::Snapshot {
            seq: 0,
            segment: SegmentRef::Live,
            id: None,
            view: View::Damage,
            info: info(),
            rows: vec![],
            total_rows: 0,
            breakdown: None,
            segment_count: 0,
            source: None,
            status: Some("waiting for a combat log…".to_string()),
        },
        DaemonMsg::SegmentList {
            seq: 9,
            entries: vec![
                ListEntry {
                    id: SegmentId(0),
                    row: list_row(false),
                },
                ListEntry {
                    id: SegmentId(u64::MAX),
                    row: list_row(true),
                },
            ],
            source: Some("log.txt".to_string()),
            active: true,
        },
        DaemonMsg::SegmentList {
            seq: 0,
            entries: vec![],
            source: None,
            active: false,
        },
        DaemonMsg::SegmentOpened { id: SegmentId(17) },
        DaemonMsg::LoadFailed {
            segment: SegmentId(2),
            error: LoadError::NotFound,
        },
        DaemonMsg::LoadFailed {
            segment: SegmentId(3),
            error: LoadError::Io("read: interrupted".to_string()),
        },
        DaemonMsg::Status {
            req_id: 1,
            game_running: true,
            source: Some("/logs/WoWCombatLog.txt".to_string()),
            clients: 3,
            linger: true,
            overlay: OverlayState::Failed("no WAYLAND_DISPLAY".to_string()),
        },
        DaemonMsg::SetVisible(true),
        DaemonMsg::Fatal("protocol mismatch".to_string()),
    ]
}

fn decode_client(frame: &[u8]) -> Result<ClientMsg, DecodeError> {
    let (tag, body, rest) = wire::split_frame(frame)?;
    if !rest.is_empty() {
        return Err(DecodeError::TrailingBytes);
    }
    ClientMsg::decode(tag, body)
}

fn decode_daemon(frame: &[u8]) -> Result<DaemonMsg, DecodeError> {
    let (tag, body, rest) = wire::split_frame(frame)?;
    if !rest.is_empty() {
        return Err(DecodeError::TrailingBytes);
    }
    DaemonMsg::decode(tag, body)
}

#[test]
fn every_client_variant_roundtrips() {
    for msg in client_msgs() {
        let frame = msg.encode();
        assert_eq!(decode_client(&frame), Ok(msg.clone()), "{msg:?}");
    }
}

#[test]
fn every_daemon_variant_roundtrips() {
    for msg in daemon_msgs() {
        let frame = msg.encode();
        assert_eq!(decode_daemon(&frame), Ok(msg.clone()), "{msg:?}");
    }
}

#[test]
fn f64_specials_survive_bit_for_bit() {
    for v in [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -0.0,
        f64::MIN_POSITIVE,
    ] {
        let mut r = row("x", None);
        r.per_sec = v;
        let snap = DaemonMsg::Snapshot {
            seq: 1,
            segment: SegmentRef::Live,
            id: None,
            view: View::Damage,
            info: info(),
            rows: vec![r],
            total_rows: 1,
            breakdown: None,
            segment_count: 1,
            source: None,
            status: None,
        };
        let decoded = decode_daemon(&snap.encode()).unwrap();
        let DaemonMsg::Snapshot { rows, .. } = decoded else {
            panic!("wrong variant");
        };
        assert_eq!(rows[0].per_sec.to_bits(), v.to_bits());
    }
}

/// Every strict prefix of every encoded frame must error — never panic,
/// never succeed. This is the fuzz the reader's bounds-checking earns.
#[test]
fn every_truncation_errors_cleanly() {
    let mut frames: Vec<Vec<u8>> = Vec::new();
    frames.extend(client_msgs().iter().map(|m| m.encode()));
    frames.extend(daemon_msgs().iter().map(|m| m.encode()));
    for frame in &frames {
        for cut in 0..frame.len() {
            let prefix = &frame[..cut];
            assert!(
                decode_client(prefix).is_err() || decode_daemon(prefix).is_err(),
                "prefix of len {cut} decoded"
            );
            // Also feed the truncated *body* straight to the message decoder:
            // the framing layer must not be the only thing standing between a
            // short buffer and a panic.
            if cut > 5 {
                let (tag, body, _) = wire::split_frame(frame).unwrap();
                let short = &body[..cut - 5];
                assert!(
                    ClientMsg::decode(tag, short).is_err()
                        && DaemonMsg::decode(tag, short).is_err(),
                    "truncated body decoded (tag {tag:#04x}, len {})",
                    short.len()
                );
            }
        }
    }
}

#[test]
fn unknown_tags_are_rejected() {
    for tag in [0x00u8, 0x06, 0x42, 0x80, 0x89, 0xFF] {
        assert_eq!(ClientMsg::decode(tag, &[]), Err(DecodeError::BadTag(tag)));
        assert_eq!(DaemonMsg::decode(tag, &[]), Err(DecodeError::BadTag(tag)));
    }
}

#[test]
fn oversized_and_zero_length_frames_are_rejected() {
    let mut oversized = Vec::new();
    wire::put_u32(&mut oversized, wire::MAX_FRAME + 1);
    oversized.extend_from_slice(&[0u8; 16]);
    assert_eq!(
        wire::split_frame(&oversized).unwrap_err(),
        DecodeError::FrameTooLarge
    );

    let mut zero = Vec::new();
    wire::put_u32(&mut zero, 0);
    assert!(wire::split_frame(&zero).is_err());

    // The stream reader refuses the same garbage as an io error.
    assert!(wire::read_frame(&mut &oversized[..]).is_err());
    assert!(wire::read_frame(&mut &zero[..]).is_err());
}

#[test]
fn trailing_bytes_are_corruption_not_slack() {
    let mut frame = ClientMsg::Shutdown.encode();
    // Grow the declared length and append a byte: decodes must refuse.
    let len = u32::from_le_bytes(frame[..4].try_into().unwrap()) + 1;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    frame.push(0xAA);
    assert_eq!(decode_client(&frame), Err(DecodeError::TrailingBytes));
}

#[test]
fn bad_bool_and_bad_utf8_are_rejected() {
    // VisibilityChanged with a bool byte of 2.
    let frame = wire::frame(0x04, &[2]);
    let (tag, body, _) = wire::split_frame(&frame).unwrap();
    assert_eq!(ClientMsg::decode(tag, body), Err(DecodeError::BadBool(2)));

    // Fatal with invalid UTF-8 in the string.
    let mut body = Vec::new();
    wire::put_u32(&mut body, 2);
    body.extend_from_slice(&[0xFF, 0xFE]);
    assert_eq!(DaemonMsg::decode(0x88, &body), Err(DecodeError::BadUtf8));
}

#[test]
fn a_lying_vec_count_is_an_error_not_an_allocation() {
    // SegmentList claiming u32::MAX rows with an empty payload.
    let mut body = Vec::new();
    wire::put_u64(&mut body, 1); // seq
    wire::put_u32(&mut body, u32::MAX); // row count lie
    assert_eq!(
        DaemonMsg::decode(0x83, &body),
        Err(DecodeError::UnexpectedEof)
    );
}

// ---- golden bytes -----------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// If any of these change, the encoded shape of the protocol changed: bump
/// `PROTO_VERSION` (which renames the socket) and re-bless the bytes.
#[test]
fn golden_bytes_pin_the_encoding() {
    assert_eq!(PROTO_VERSION, 6, "bumped? re-bless the golden bytes below");

    let hello = ClientMsg::Hello {
        proto: 1,
        client: ClientKind::Tui,
        pid: 0x0102_0304,
    };
    assert_eq!(hex(&hello.encode()), "080000000101000004030201");

    let watch = ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Id(SegmentId(2)),
        view: View::Healing,
        top_n: Some(5),
        drill: Some("Ana".to_string()),
    });
    assert_eq!(
        hex(&watch.encode()),
        "1900000002010102000000000000000101050000000103000000416e61"
    );

    let snap = DaemonMsg::Snapshot {
        seq: 7,
        segment: SegmentRef::Live,
        id: Some(SegmentId(9)),
        view: View::Damage,
        info: SegmentInfo {
            kind: SegmentKind::Encounter,
            name: "B".to_string(),
            start_ms: 1000,
            duration_ms: 2000,
            success: Some(true),
            live: true,
            instance: None,
            pars_ms: None,
        },
        rows: vec![Row {
            key: "K".to_string(),
            label: "L".to_string(),
            amount: 10,
            extra: 0,
            count: 3,
            crits: 1,
            per_sec: 1.5,
            pct: 50.0,
            class: Some(Class::Mage),
            spec: Some(Spec::FrostMage), // specID 64 -> 4000 little-endian
            hp: Some((5, 6)),
            gain: true,
        }],
        total_rows: 1,
        breakdown: None,
        segment_count: 2,
        source: None,
        status: None,
    };
    // v5: SegmentInfo gained a trailing Option<u32> `instance` (R10) — the
    // `00` presence byte right after the `live` flag. v6: a trailing
    // Option<(i64, i64, i64)> `pars_ms` (keystone timers) after `instance`.
    assert_eq!(
        hex(&snap.encode()),
        "8e0000008207000000000000000001090000000000000000000100000042e803000000000000d0070000000000\
         00010101000001000000010000004b010000004c0a00000000000000000000000000000000000000000\
         0f83f000000000000494001074000030000000000000001000000000000000105000000000000000600\
         00000000000001010000000002000000000\
         0"
    );
}
