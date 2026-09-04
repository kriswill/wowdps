//! Codec suite: every variant roundtrips, every truncation errors (never
//! panics), garbage is rejected, and golden bytes force a conscious
//! `PROTO_VERSION` bump whenever an encoded shape changes.

use wowdps_model::{
    Class, Encounter, GearItem, ListRow, Loadout, Mark, MarkKind, MissKind, Mitigation, Row,
    SegmentId, SegmentInfo, SegmentKind, Spec, TalentPick, Timeline, View,
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
                after_id: Some("2f53c7079010c5a2-1788380107617".to_string()),
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
                after_id: None,
            },
        },
        ClientMsg::GetHistory {
            req_id: 3,
            query: HistoryQuery::Progression {
                encounter: 3130,
                difficulty: 16,
                local_cutover_hour: Some(6),
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
                local_cutover_hour: None,
            },
        },
        ClientMsg::GetFight {
            req_id: 5,
            fight_id: "0123456789abcdef-1722000000123".to_string(),
            view: View::Deaths,
            drill: Some("Player-1-A".to_string()),
            boss: Some("Vexamus".to_string()),
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
        ClientMsg::Regrade {
            req_id: 8,
            fight_id: Some("x-1".to_string()),
            encounter: None,
            difficulty: None,
            kind: Some(FightKind::Key),
        },
        ClientMsg::Regrade {
            req_id: 9,
            fight_id: None,
            encounter: Some(3429),
            difficulty: Some(14),
            kind: Some(FightKind::Key),
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
        bosses: vec![wowdps_proto::history::KeyBoss {
            name: "Vexamus".to_string(),
            encounter: Some(Encounter {
                id: 2562,
                difficulty: 8,
                group_size: 5,
            }),
            start_utc_ms: 1_722_000_000_500,
            duration_ms: 60_000,
            success: Some(true),
        }],
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
                // v21 (R17): the mitigation record rides the drill.
                mitigation: Some(mitigation()),
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
            log_id: None,
        },
        DaemonMsg::SegmentList {
            seq: 0,
            entries: vec![],
            source: None,
            active: false,
            log_id: None,
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
            answer: HistoryAnswer::Fights {
                cards: vec![card(), FightCard::default()],
                total: 7,
            },
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
                        kills: 1,
                        best_pct: Some(37),
                        tz_min: Some(-420),
                    },
                    Night {
                        day_utc_ms: -86_400_000,
                        pulls: 10,
                        kill: true,
                        kills: 1,
                        best_pct: None,
                        tz_min: Some(-420),
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
                tz_min: None,
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
        DaemonMsg::History {
            req_id: 6,
            answer: HistoryAnswer::Regraded { queued: 2 },
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
                    mitigation: None,
                }),
                tier: 3,
                has_recap: true,
                loadout: Some(Loadout::default()),
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
    // v20 took 0x08–0x0C (history one-shots, Regrade last) and 0x8B–0x8D
    // (their replies).
    for tag in [0x00u8, 0x0D, 0x42, 0x80, 0x8E, 0xFF] {
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
    assert_eq!(PROTO_VERSION, 21, "bumped? re-bless the golden bytes below");

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
        boss: None,
    };
    assert_eq!(
        hex(&get_fight.encode()),
        "0f0000000905000000 03000000782d31 05 00 00".replace(' ', "")
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

    // v21 (R17): View gained `Taken` (code 6) and Breakdown a trailing
    // Option<Mitigation>. The mitigation is seven u64 amounts in
    // declaration order, then the ten miss counts as u32 in MissKind::ALL
    // order — every field distinct so the byte order is proven.
    let taken = DaemonMsg::Snapshot {
        seq: 1,
        segment: SegmentRef::Live,
        id: None,
        view: View::Taken,
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
        rows: vec![],
        total_rows: 0,
        breakdown: Some(Breakdown {
            by_spell: vec![],
            by_target: vec![],
            timeline: None,
            spell_timeline: None,
            spell_targets: None,
            mitigation: Some(mitigation()),
        }),
        segment_count: 0,
        source: None,
        status: None,
    };
    assert_eq!(
        hex(&taken.encode()),
        // len 0xa2 | 82 | seq 1 | Live 00 | id None 00 | view 06 | info (27
        // bytes: Trash 01, "" 00000000, start 0, duration 0, success 00,
        // live 00, instance 00, pars 00, arena 00, encounter 00) | rows 0 |
        // total_rows 0 | breakdown 01: by_spell 0, by_target 0, timeline 00,
        // spell_timeline 00, spell_targets 00, mitigation 01 + 7×u64 (1..7)
        // + 10×u32 (0x11..0x1a, Dodge first, Resist last) | segment_count 0
        // | source 00 | status 00.
        "a20000008201000000000000000000060100000000000000000000000000000000000000000000000000000000000000\
         000000010000000000000000000000010100000000000000020000000000000003000000000000000400000000000000\
         050000000000000006000000000000000700000000000000110000001200000013000000140000001500000016000000\
         1700000018000000190000001a000000000000000000"
    );
}

/// v21: every field non-zero and distinct.
fn mitigation() -> Mitigation {
    let mut m = Mitigation {
        absorbed: 1,
        blocked: 2,
        absorbed_full: 3,
        blocked_full: 4,
        overkill: 5,
        stagger: 6,
        stagger_ticked: 7,
        misses: [0; MissKind::COUNT],
    };
    for (i, kind) in MissKind::ALL.iter().enumerate() {
        if let Some(slot) = m.misses.get_mut(kind.index()) {
            *slot = 0x11 + i as u32;
        }
    }
    m
}

/// v21: the mitigation record is a fixed 96 bytes behind its presence byte,
/// and a Breakdown whose presence byte is 0 decodes to `None` — proven by
/// diffing the `Some` and `None` encodings of otherwise identical snapshots.
#[test]
fn v21_mitigation_is_96_bytes_behind_a_presence_byte_and_none_decodes_to_none() {
    let make = |mitigation: Option<Mitigation>| DaemonMsg::Snapshot {
        seq: 3,
        segment: SegmentRef::Id(SegmentId(4)),
        id: Some(SegmentId(4)),
        view: View::Taken,
        info: info(),
        rows: vec![row("Tank", Some(Class::Warrior))],
        total_rows: 1,
        breakdown: Some(Breakdown {
            by_spell: vec![row("Melee", None)],
            by_target: vec![row("Boss", None)],
            timeline: None,
            spell_timeline: None,
            spell_targets: None,
            mitigation,
        }),
        segment_count: 5,
        source: Some("x.txt".to_string()),
        status: None,
    };
    let some = make(Some(mitigation())).encode();
    let none = make(None).encode();
    assert_eq!(some.len(), none.len() + 7 * 8 + 10 * 4);
    // Both end with segment_count (u32 5) + source + status: 4 + 1+4+5 + 1.
    let tail = 4 + 10 + 1;
    let (some_head, some_tail) = some.split_at(some.len() - tail);
    let (none_head, none_tail) = none.split_at(none.len() - tail);
    assert_eq!(some_tail, none_tail);
    // Frame lengths differ by 96; everything else up to the presence byte
    // is byte-identical.
    assert_eq!(
        &some_head[4..none_head.len() - 1],
        &none_head[4..none_head.len() - 1]
    );
    assert_eq!(none_head[none_head.len() - 1], 0, "None = presence byte 0");
    assert_eq!(some_head[none_head.len() - 1], 1, "Some = presence byte 1");
    let m = &some_head[none_head.len()..];
    assert_eq!(m.len(), 96);
    assert_eq!(&m[..8], &1u64.to_le_bytes());
    assert_eq!(&m[48..56], &7u64.to_le_bytes());
    assert_eq!(&m[56..60], &0x11u32.to_le_bytes(), "Dodge first");
    assert_eq!(&m[92..96], &0x1au32.to_le_bytes(), "Resist last");

    for (frame, want) in [(&some, Some(mitigation())), (&none, None)] {
        let Ok(DaemonMsg::Snapshot {
            breakdown: Some(b), ..
        }) = decode_daemon(frame)
        else {
            panic!("decode failed");
        };
        assert_eq!(b.mitigation, want);
    }
    // Truncating anywhere inside the record is an error, never a panic.
    let body_end = some.len() - tail;
    for cut in (body_end - 96)..body_end {
        assert!(decode_daemon(&some[..cut]).is_err(), "cut at {cut}");
    }
}
