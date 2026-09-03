//! Codec suite: every variant roundtrips, every truncation errors (never
//! panics), garbage is rejected, and golden bytes force a conscious
//! `PROTO_VERSION` bump whenever an encoded shape changes.

use wowdps_model::{
    Class, Encounter, GearItem, ListRow, Loadout, Mark, MarkKind, Row, SegmentId, SegmentInfo,
    SegmentKind, Spec, TalentPick, Timeline, View,
};
use wowdps_proto::history::{CardPlayer, FightCard, FightKind, KeyInfo};
use wowdps_proto::wire::{self, DecodeError};
use wowdps_proto::{
    Breakdown, ClientKind, ClientMsg, CompareSide, Cursor, DaemonMsg, FightSort, HistoryAnswer,
    HistoryQuery, HistoryStatus, ListEntry, LoadError, Night, OverlayState, PROTO_VERSION,
    SegmentRef, StoredFight, TrendBucket, TrendPoint,
};

/// R12: one comparison side, with every marker kind represented.
fn compare_side(guid: &str) -> CompareSide {
    CompareSide {
        guid: guid.to_string(),
        total: row(guid, Some(Class::Mage)),
        spells: vec![row("Frostbolt", Some(Class::Mage))],
        // v18: the ability drill's curve for this side — exercise the arm.
        spell_timeline: Some(Timeline {
            bucket_ms: 1000,
            buckets: vec![3, 0, 4],
            marks: vec![],
        }),
        timeline: Timeline {
            bucket_ms: 1000,
            buckets: vec![0, u64::MAX, 42],
            marks: vec![
                Mark {
                    at_ms: i64::MIN,
                    kind: MarkKind::TrinketUse,
                    label: "Sigil «of» Ruin".to_string(),
                    spell_id: u32::MAX,
                    dur_ms: i64::MAX,
                },
                Mark {
                    at_ms: 0,
                    kind: MarkKind::TrinketProc,
                    label: String::new(),
                    spell_id: 0,
                    dur_ms: 0,
                },
                Mark {
                    at_ms: i64::MAX,
                    kind: MarkKind::Consumable,
                    label: "Tempered Potion".to_string(),
                    spell_id: 1_282_741,
                    dur_ms: 30_000,
                },
                // v13: the external-buff arm.
                Mark {
                    at_ms: 42,
                    kind: MarkKind::External,
                    label: "Bloodlust".to_string(),
                    spell_id: 2825,
                    dur_ms: 40_000,
                },
            ],
        },
    }
}

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
        // v9: exercise both arms — classed rows carry a spell id.
        spell_id: if class.is_some() { 116 } else { 0 },
        // v10 (R13): exercise both arms of the team flag.
        enemy: class.is_none(),
        school: 0x24, // v15: Shadowflame, the combo arm
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
        // v11 (R13): the arm the WIN/LOSS wording hangs off.
        arena: true,
        encounter: None,
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
        arena: false,
        encounter: None,
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
            spell: None,
        }),
        ClientMsg::Watch(Cursor::Segment {
            segment: SegmentRef::Id(SegmentId(u64::MAX)),
            view: View::Deaths,
            top_n: Some(0),
            drill: Some("Player-1301-0AB7C3D2".to_string()),
            spell: Some("Chaos Bolt".to_string()),
        }),
        ClientMsg::Watch(Cursor::Compare {
            segment: SegmentRef::Id(SegmentId(0)),
            a: "Player-1301-0AB7C3D2".to_string(),
            b: "Player-1301-0AB7C3D3".to_string(),
            // v12: exercise the windowed arm; the golden pins `None`.
            range: Some((0, u32::MAX)),
            spell: Some("Chaos Bolt".to_string()),
        }),
        ClientMsg::GetStatus { req_id: 42 },
        ClientMsg::VisibilityChanged { visible: false },
        ClientMsg::Shutdown,
        ClientMsg::DiscardTrash,
        // v19: the one-shot loadout query.
        ClientMsg::GetLoadout {
            req_id: u32::MAX,
            segment: SegmentRef::Id(SegmentId(7)),
            guid: "Player-1301-0AB7C3D2".to_string(),
        },
        ClientMsg::GetLoadout {
            req_id: 0,
            segment: SegmentRef::Live,
            guid: String::new(),
        },
        // v20: the history one-shots, every query variant and edge value.
        ClientMsg::GetHistory {
            req_id: 1,
            query: HistoryQuery::Fights {
                encounter: Some(3130),
                difficulty: Some(15),
                guid: Some("Player-1-A".to_string()),
                since_utc_ms: Some(-1),
                kind: Some(FightKind::Key),
                sort: FightSort::OwnerPerSec,
                limit: u32::MAX,
            },
        },
        ClientMsg::GetHistory {
            req_id: 2,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                guid: None,
                since_utc_ms: None,
                kind: None,
                sort: FightSort::Fastest,
                limit: 0,
            },
        },
        ClientMsg::GetHistory {
            req_id: 3,
            query: HistoryQuery::Progression {
                encounter: 3130,
                difficulty: 16,
            },
        },
        ClientMsg::GetHistory {
            req_id: 4,
            query: HistoryQuery::Trend {
                guid: "Player-1-A".to_string(),
                spec: Some(64),
                encounter: None,
                difficulty: Some(15),
                view: View::Healing,
                bucket: TrendBucket::Week,
                since_utc_ms: None,
                limit: 7,
            },
        },
        ClientMsg::GetFight {
            req_id: 5,
            fight_id: "0123456789abcdef-1722000000123".to_string(),
            view: View::Deaths,
            drill: Some("Player-1-A".to_string()),
        },
        ClientMsg::PinFight {
            req_id: 6,
            fight_id: "x-1".to_string(),
            pinned: true,
        },
        ClientMsg::ImportLog {
            req_id: 7,
            path: "/games/wow/Logs".to_string(),
        },
    ]
}

/// A fully populated card for the history payload round trips.
fn card() -> FightCard {
    FightCard {
        schema: 1,
        id: "0123456789abcdef-1722000000123".to_string(),
        log: 0x0123_4567_89ab_cdef,
        content: u64::MAX,
        kind: FightKind::Key,
        name: "Skyreach +10".to_string(),
        encounter: Some(Encounter {
            id: 3130,
            difficulty: 15,
            group_size: 20,
        }),
        key: Some(KeyInfo {
            map_id: 1209,
            difficulty: 23,
            level: Some(10),
            completed: Some(false),
        }),
        start_local_ms: 1_722_000_000_123,
        tz_min: Some(-240),
        start_utc_ms: 1_722_014_400_123,
        duration_ms: 61_500,
        official_ms: Some(61_400),
        pars_ms: Some((2_040_000, 1_632_000, 1_224_000)),
        success: Some(true),
        aborted: false,
        build: (12, 0, 2),
        project_id: 1,
        log_version: 22,
        owner: Some("Player-1-A".to_string()),
        byte_range: Some((10, u64::MAX)),
        pinned: true,
        best_pct: Some(37),
        players: vec![
            CardPlayer {
                guid: "Player-1-A".to_string(),
                name: "Ana-Realm".to_string(),
                class: Some(Class::Mage),
                spec: Some(Spec::FrostMage),
                loadout: Some(0x00ff_00ff_00ff_00ff),
                logged: true,
                enemy: false,
                damage: 123_456,
                dps: 2007.4,
                healing: 0,
                hps: 0.0,
                deaths: 1,
            },
            CardPlayer::default(),
        ],
    }
}

/// Every DaemonMsg variant, edge values included.
fn daemon_msgs() -> Vec<DaemonMsg> {
    vec![
        DaemonMsg::HelloAck {
            proto: PROTO_VERSION,
            version: "0.1.0".to_string(),
        },
        DaemonMsg::CompareSnapshot {
            seq: u64::MAX,
            segment: SegmentRef::Live,
            id: None,
            info: info(),
            a: Box::new(compare_side("Player-1-A")),
            b: Box::new(CompareSide::default()),
            // v12: the answered window rides along; exercise the Some arm.
            range: Some((15_000, 45_000)),
            source: None,
            status: Some("loading…".to_string()),
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
                // v14: the drilled player's damage timeline rides along.
                timeline: Some(Timeline {
                    bucket_ms: 1000,
                    buckets: vec![0, 42, u64::MAX],
                    marks: vec![Mark {
                        at_ms: 1500,
                        kind: MarkKind::TrinketUse,
                        label: "Signet".to_string(),
                        spell_id: 11,
                        dur_ms: 20_000,
                    }],
                }),
                // v16: the drilled ability's own curve rides along too.
                spell_timeline: Some(Timeline {
                    bucket_ms: 1000,
                    buckets: vec![7, 0, 9],
                    marks: vec![],
                }),
                // v17: and who the ability landed on.
                spell_targets: Some(vec![row("Boss", None)]),
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
            // v20: the history store's state rides along.
            history: HistoryStatus {
                enabled: true,
                fights: 4_000,
                dropped: 1,
                importing: 2,
                owner_inferred: true,
                error: Some("ENOSPC".to_string()),
            },
        },
        DaemonMsg::SetVisible(true),
        DaemonMsg::Fatal("protocol mismatch".to_string()),
        // v19: the loadout reply — a rich build and the not-known case.
        DaemonMsg::Loadout {
            req_id: u32::MAX,
            guid: "Player-1301-0AB7C3D2".to_string(),
            loadout: Some(Loadout {
                spec_id: Some(265),
                talents: vec![
                    TalentPick {
                        node_id: 91024,
                        entry_id: 124871,
                        rank: 1,
                    },
                    TalentPick {
                        node_id: 91026,
                        entry_id: 124873,
                        rank: 0,
                    },
                ],
                gear: vec![
                    GearItem {
                        item_id: 212446,
                        ilvl: 639,
                        enchants: vec![7445],
                        bonus_ids: vec![6652, 10356],
                        gems: vec![213743, 213743],
                    },
                    GearItem::default(),
                ],
            }),
        },
        DaemonMsg::Loadout {
            req_id: 0,
            guid: String::new(),
            loadout: None,
        },
        // v20: the history replies, every answer variant.
        DaemonMsg::History {
            req_id: 1,
            answer: HistoryAnswer::Fights(vec![card(), FightCard::default()]),
        },
        DaemonMsg::History {
            req_id: 2,
            answer: HistoryAnswer::Progression {
                pulls: 40,
                kills: 2,
                first_kill: Some(Box::new(card())),
                nights: vec![
                    Night {
                        day_utc_ms: 1_722_000_000_000,
                        pulls: 30,
                        kill: false,
                        best_pct: Some(37),
                    },
                    Night {
                        day_utc_ms: -86_400_000,
                        pulls: 10,
                        kill: true,
                        best_pct: None,
                    },
                ],
                median_kill_ms: Some(61_500),
            },
        },
        DaemonMsg::History {
            req_id: 3,
            answer: HistoryAnswer::Progression {
                pulls: 0,
                kills: 0,
                first_kill: None,
                nights: Vec::new(),
                median_kill_ms: None,
            },
        },
        DaemonMsg::History {
            req_id: 4,
            answer: HistoryAnswer::Trend(vec![TrendPoint {
                bucket_utc_ms: 1_722_000_000_000,
                fight_id: "x-1".to_string(),
                spec: Some(64),
                amount: 5,
                per_sec: 0.5,
                duration_ms: 10_000,
                n: 3,
            }]),
        },
        DaemonMsg::History {
            req_id: 5,
            answer: HistoryAnswer::Pinned {
                fight_id: "x-1".to_string(),
                pinned: false,
            },
        },
        DaemonMsg::History {
            req_id: 6,
            answer: HistoryAnswer::Imported { queued: 9 },
        },
        DaemonMsg::Fight {
            req_id: 7,
            fight: Some(StoredFight {
                card: card(),
                rows: vec![row("K", Some(Class::Mage))],
                breakdown: Some(Breakdown {
                    by_spell: vec![row("Frostbolt", None)],
                    by_target: Vec::new(),
                    timeline: Some(Timeline {
                        bucket_ms: 1000,
                        buckets: vec![1, 2],
                        marks: Vec::new(),
                    }),
                    spell_timeline: None,
                    spell_targets: None,
                }),
            }),
        },
        DaemonMsg::Fight {
            req_id: 8,
            fight: None,
        },
        DaemonMsg::HistoryChanged {
            fight_id: "x-1".to_string(),
        },
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
    // 0x89 was free until v8 gave it to CompareSnapshot (R12); 0x07/0x8A
    // were free until v19 gave them to GetLoadout/Loadout.
    // v20 took 0x08–0x0B (history one-shots) and 0x8B–0x8D (their replies).
    for tag in [0x00u8, 0x0C, 0x42, 0x80, 0x8E, 0xFF] {
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
    assert_eq!(PROTO_VERSION, 20, "bumped? re-bless the golden bytes below");

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
        // v16: the ability drill's key rides the cursor.
        spell: Some("Fireball".to_string()),
    });
    assert_eq!(
        hex(&watch.encode()),
        // v16: Cursor::Segment gained a trailing Option<String> spell.
        "2600000002010102000000000000000101050000000103000000416e6101080000004669726562616c6c"
    );

    // v8 (R12): Cursor gained the `Compare` arm, code 2.
    let compare_watch = ClientMsg::Watch(Cursor::Compare {
        segment: SegmentRef::Live,
        a: "A".to_string(),
        b: "Bo".to_string(),
        // v12: the window rides the cursor; `None` keeps the golden minimal —
        // the roundtrip suite covers the Some arm.
        range: None,
        spell: None,
    });
    assert_eq!(
        hex(&compare_watch.encode()),
        "10000000020200010000004102000000426f0000"
    );

    // v8 (R12): DaemonMsg gained `CompareSnapshot`, tag 0x89. A side is
    // guid + total Row + spell Rows + Timeline (bucket_ms, buckets, marks).
    let compare = DaemonMsg::CompareSnapshot {
        seq: 1,
        segment: SegmentRef::Live,
        id: None,
        info: SegmentInfo {
            kind: SegmentKind::Trash,
            name: String::new(),
            start_ms: 0,
            duration_ms: 0,
            success: None,
            live: false,
            instance: None,
            pars_ms: None,
            arena: false,
            encounter: None,
        },
        a: Box::new(CompareSide {
            guid: "A".to_string(),
            total: Row::default(),
            spells: Vec::new(),
            spell_timeline: None,
            timeline: Timeline {
                bucket_ms: 1000,
                buckets: vec![5],
                marks: vec![Mark {
                    at_ms: 250,
                    kind: MarkKind::Consumable,
                    label: "P".to_string(),
                    spell_id: 7,
                    dur_ms: 9,
                }],
            },
        }),
        b: Box::new(CompareSide::default()),
        range: None,
        source: None,
        status: None,
    };
    assert_eq!(
        hex(&compare.encode()),
        // v15: each Row grew a trailing u32 school — the two zeroed Rows here
        // add four `00` bytes apiece.
        // v18: each side grew a trailing Option<Timeline> spell_timeline —
        // the two `00` presence bytes at the tail of each zeroed side.
        // v20: SegmentInfo grew a trailing Option<Encounter> — the `00`
        // presence byte right after the `arena` flag.
        "020100008901000000000000000000010000000000000000000000000000000000000000000000000000010000004100\
         000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000\
         00000000000000000000000000000000000000000000000000e803000001000000050000000000000001000000fa0000\
         000000000002010000005007000000090000000000000000000000000000000000000000000000000000000000000000\
         000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000\
         00000000000000000000000000000000000000000000"
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
            arena: false,
            encounter: None,
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
            spell_id: 30451,
            enemy: true,
            school: 32, // Shadow — 0x20 in the golden bytes
        }],
        total_rows: 1,
        breakdown: None,
        segment_count: 2,
        source: None,
        status: None,
    };
    // v19: the loadout pair. GetLoadout is req_id + SegmentRef + guid …
    let get_loadout = ClientMsg::GetLoadout {
        req_id: 3,
        segment: SegmentRef::Live,
        guid: "G".to_string(),
    };
    assert_eq!(hex(&get_loadout.encode()), "0b0000000703000000000100000047");

    // … and Loadout answers with req_id + echoed guid + Option<Loadout>
    // (spec u16 raw id, talent picks as three u32s, gear items as two u32s
    // plus three u32 lists).
    let loadout = DaemonMsg::Loadout {
        req_id: 3,
        guid: "G".to_string(),
        loadout: Some(Loadout {
            spec_id: Some(71),
            talents: vec![TalentPick {
                node_id: 1,
                entry_id: 2,
                rank: 3,
            }],
            gear: vec![GearItem {
                item_id: 9,
                ilvl: 10,
                enchants: vec![],
                bonus_ids: vec![11],
                gems: vec![],
            }],
        }),
    };
    assert_eq!(
        hex(&loadout.encode()),
        "390000008a0300000001000000470147000100000001000000020000000300000001000000090000000a0000\
         0000000000010000000b00000000000000"
    );

    // v20: the history one-shots. The small ones are pinned byte for byte;
    // the card-carrying ones are covered by the round trip (a card is ~40
    // fields) and by CONTRACT's field order.
    let pin = ClientMsg::PinFight {
        req_id: 6,
        fight_id: "x-1".to_string(),
        pinned: true,
    };
    assert_eq!(hex(&pin.encode()), "0d0000000a0600000003000000782d3101");
    let import = ClientMsg::ImportLog {
        req_id: 7,
        path: "/l".to_string(),
    };
    assert_eq!(hex(&import.encode()), "0b0000000b07000000020000002f6c");
    let get_fight = ClientMsg::GetFight {
        req_id: 5,
        fight_id: "x-1".to_string(),
        view: View::Deaths,
        drill: None,
    };
    assert_eq!(
        hex(&get_fight.encode()),
        "0e0000000905000000 03000000782d31 05 00".replace(' ', "")
    );
    let changed = DaemonMsg::HistoryChanged {
        fight_id: "x-1".to_string(),
    };
    assert_eq!(hex(&changed.encode()), "080000008d03000000782d31");
    let imported = DaemonMsg::History {
        req_id: 7,
        answer: HistoryAnswer::Imported { queued: 9 },
    };
    assert_eq!(hex(&imported.encode()), "0a0000008b070000000409000000");

    // v5: SegmentInfo gained a trailing Option<u32> `instance` (R10) — the
    // `00` presence byte right after the `live` flag. v6: a trailing
    // Option<(i64, i64, i64)> `pars_ms` (keystone timers) after `instance`.
    // v10 (R13): Row gained a trailing `enemy` bool — the `01` right after
    // the spell id.
    assert_eq!(
        hex(&snap.encode()),
        // v15: Row gained a trailing u32 `school` — the `20000000` (Shadow,
        // 0x20) right after the `enemy` flag.
        // v20: SegmentInfo gained a trailing Option<Encounter> — the `00`
        // presence byte right after the `arena` flag.
        "990000008207000000000000000001090000000000000000000100000042e803000000000000d0070000000000000101\
         010000000001000000010000004b010000004c0a000000000000000000000000000000000000000000f83f0000000000\
         0049400107400003000000000000000100000000000000010500000000000000060000000000000001f3760000012000\
         00000100000000020000000000"
    );
}
