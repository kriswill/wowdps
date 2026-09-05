//! Every enum code the wire carries roundtrips — views, classes, specs,
//! client kinds, load errors, overlay states — so a variant added to the
//! model without a code (or with a colliding one) fails here, not in a
//! frontend that suddenly decodes the wrong thing.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use wowdps_model::{Class, Role, Row, SegmentInfo, SegmentKind, Spec, View};
use wowdps_proto::wire;
use wowdps_proto::{
    ClientKind, ClientMsg, DaemonMsg, FightSort, HistoryQuery, HistoryStatus, LoadError,
    OverlayState, SegmentRef, TrendBucket, TrendMeasure,
};

fn roundtrip_daemon(msg: &DaemonMsg) -> DaemonMsg {
    let frame = msg.encode();
    let (tag, body) = wire::read_frame(&mut &frame[..]).expect("a whole frame");
    DaemonMsg::decode(tag, &body).expect("decodes")
}

fn roundtrip_client(msg: &ClientMsg) -> ClientMsg {
    let frame = msg.encode();
    let (tag, body) = wire::read_frame(&mut &frame[..]).expect("a whole frame");
    ClientMsg::decode(tag, &body).expect("decodes")
}

fn snapshot(view: View, rows: Vec<Row>) -> DaemonMsg {
    DaemonMsg::Snapshot {
        seq: 1,
        segment: SegmentRef::Live,
        id: None,
        view,
        info: SegmentInfo {
            kind: SegmentKind::Trash,
            name: String::new(),
            start_ms: 0,
            duration_ms: 0,
            success: None,
            live: true,
            instance: None,
            pars_ms: None,
            arena: false,
            encounter: None,
        },
        rows,
        total_rows: 0,
        breakdown: None,
        segment_count: 0,
        source: None,
        status: None,
    }
}

const CLASSES: [Class; 13] = [
    Class::Warrior,
    Class::Paladin,
    Class::Hunter,
    Class::Rogue,
    Class::Priest,
    Class::DeathKnight,
    Class::Shaman,
    Class::Mage,
    Class::Warlock,
    Class::Monk,
    Class::Druid,
    Class::DemonHunter,
    Class::Evoker,
];

/// Every specialization id the game has (ChrSpecialization), by class.
const SPEC_IDS: [u32; 40] = [
    71, 72, 73, 65, 66, 70, 253, 254, 255, 259, 260, 261, 256, 257, 258, 250, 251, 252, 262, 263,
    264, 62, 63, 64, 265, 266, 267, 268, 270, 269, 102, 103, 104, 105, 577, 581, 1467, 1468, 1473,
    1473,
];

#[test]
fn every_view_roundtrips() {
    for view in [
        View::Damage,
        View::Healing,
        View::Interrupts,
        View::CrowdControl,
        View::Dispels,
        View::Deaths,
        View::Taken,
    ] {
        let msg = snapshot(view, Vec::new());
        assert_eq!(roundtrip_daemon(&msg), msg, "{view:?}");
    }
}

#[test]
fn every_class_and_spec_roundtrips_on_a_row() {
    let rows: Vec<Row> = CLASSES
        .iter()
        .map(|&class| Row {
            key: format!("{class:?}"),
            class: Some(class),
            ..Row::default()
        })
        .chain(SPEC_IDS.iter().map(|&id| {
            let spec = Spec::from_id(id).unwrap_or_else(|| panic!("spec {id} is known"));
            Row {
                key: spec.name().to_string(),
                class: Some(spec.class()),
                spec: Some(spec),
                ..Row::default()
            }
        }))
        .collect();
    let msg = snapshot(View::Damage, rows.clone());
    assert_eq!(roundtrip_daemon(&msg), msg);

    // The model's own tables agree with each other.
    for &id in &SPEC_IDS {
        let spec = Spec::from_id(id).unwrap();
        assert_eq!(spec.id(), id, "{spec:?}");
        assert_eq!(Class::from_spec(id), Some(spec.class()));
        assert!(!spec.name().is_empty());
        assert!(CLASSES.contains(&spec.class()));
    }
    assert_eq!(Spec::from_id(0), None);
    assert_eq!(Class::from_spec(0), None);
    for class in CLASSES {
        let (r, g, b) = class.rgb();
        assert!(r > 0 || g > 0 || b > 0, "{class:?} has a class color");
    }
}

#[test]
fn every_client_kind_roundtrips() {
    for kind in [
        ClientKind::Tui,
        ClientKind::Window,
        ClientKind::Overlay,
        ClientKind::Mcp,
    ] {
        let msg = ClientMsg::Hello {
            proto: 7,
            client: kind,
            pid: 42,
        };
        assert_eq!(roundtrip_client(&msg), msg, "{kind:?}");
    }
}

/// v22: every `TrendMeasure` rides `Trend`, every `Role` (and `None`)
/// rides `Fights`; the names round-trip too, and a code past the last
/// variant is a `BadTag`.
#[test]
fn every_trend_measure_and_role_roundtrips() {
    let measures = [
        TrendMeasure::Dps,
        TrendMeasure::Hps,
        TrendMeasure::Dtps,
        TrendMeasure::MitigatedPct,
        // v23 (R19, step 3b).
        TrendMeasure::EffectiveDps,
    ];
    for measure in measures {
        let msg = ClientMsg::GetHistory {
            req_id: 1,
            query: HistoryQuery::Trend {
                guid: "g".to_string(),
                spec: None,
                encounter: None,
                difficulty: None,
                measure,
                bucket: TrendBucket::None,
                since_utc_ms: None,
                limit: 0,
                local_cutover_hour: None,
            },
        };
        assert_eq!(roundtrip_client(&msg), msg, "{measure:?}");
        assert_eq!(TrendMeasure::from_name(measure.name()), Some(measure));
    }
    assert_eq!(TrendMeasure::from_name("taken"), None);
    assert_eq!(TrendMeasure::from_name("DPS"), None, "names are lower-case");
    for role in [None, Some(Role::Tank), Some(Role::Healer), Some(Role::Dps)] {
        let msg = ClientMsg::GetHistory {
            req_id: 2,
            query: HistoryQuery::Fights {
                encounter: None,
                difficulty: None,
                guid: None,
                since_utc_ms: None,
                kind: None,
                sort: FightSort::Newest,
                limit: 0,
                after_id: None,
                role,
            },
        };
        assert_eq!(roundtrip_client(&msg), msg, "{role:?}");
    }
    // The byte after the last variant is rejected in both places.
    let mut frame = ClientMsg::GetHistory {
        req_id: 2,
        query: HistoryQuery::Fights {
            encounter: None,
            difficulty: None,
            guid: None,
            since_utc_ms: None,
            kind: None,
            sort: FightSort::Newest,
            limit: 0,
            after_id: None,
            role: Some(Role::Dps),
        },
    }
    .encode();
    let last = frame.len() - 1;
    frame[last] = 3;
    let (tag, body) = wire::read_frame(&mut &frame[..]).expect("a whole frame");
    assert!(
        matches!(
            ClientMsg::decode(tag, &body),
            Err(wire::DecodeError::BadTag(3))
        ),
        "role 3"
    );
    let mut frame = ClientMsg::GetHistory {
        req_id: 1,
        query: HistoryQuery::Trend {
            guid: String::new(),
            spec: None,
            encounter: None,
            difficulty: None,
            measure: TrendMeasure::EffectiveDps,
            bucket: TrendBucket::None,
            since_utc_ms: None,
            limit: 0,
            local_cutover_hour: None,
        },
    }
    .encode();
    // …| guid 00000000 | spec 00 | enc 00 | diff 00 | MEASURE | bucket 00 |
    // since 00 | limit 00000000 | cutover 00: the measure byte is 8 from the
    // end. v23: EffectiveDps is code 4, so 5 is the first bad code.
    let at = frame.len() - 8;
    assert_eq!(frame[at], 4);
    frame[at] = 5;
    let (tag, body) = wire::read_frame(&mut &frame[..]).expect("a whole frame");
    assert!(
        matches!(
            ClientMsg::decode(tag, &body),
            Err(wire::DecodeError::BadTag(5))
        ),
        "measure 5"
    );
}

#[test]
fn every_load_error_and_overlay_state_roundtrips() {
    for error in [
        LoadError::NotFound,
        LoadError::Rotated,
        LoadError::Io("disk gone".to_string()),
    ] {
        let msg = DaemonMsg::LoadFailed {
            segment: wowdps_model::SegmentId(3),
            error,
        };
        assert_eq!(roundtrip_daemon(&msg), msg);
    }
    for overlay in [
        OverlayState::Absent,
        OverlayState::Visible,
        OverlayState::Hidden,
        OverlayState::Failed("no WAYLAND_DISPLAY".to_string()),
    ] {
        let msg = DaemonMsg::Status {
            req_id: 9,
            game_running: true,
            source: Some("logs:/x".to_string()),
            clients: 2,
            linger: true,
            overlay,
            history: HistoryStatus::default(),
        };
        assert_eq!(roundtrip_daemon(&msg), msg);
    }
}

/// v24 (R18): every `MarkKind` rides a timeline mark together with its
/// caster, and the code past the last variant is a `BadTag` — never a mark
/// silently re-kinded on the way through.
#[test]
fn every_mark_kind_roundtrips_with_its_caster() {
    use wowdps_model::{Mark, MarkKind, Timeline};
    use wowdps_proto::{CompareSide, DecodeError};

    const AT: i64 = 0x0102_0304_0506_0708;
    let compare = |kind: MarkKind| DaemonMsg::CompareSnapshot {
        seq: 1,
        segment: SegmentRef::Live,
        id: None,
        info: SegmentInfo {
            kind: SegmentKind::Trash,
            name: String::new(),
            start_ms: 0,
            duration_ms: 0,
            success: None,
            live: true,
            instance: None,
            pars_ms: None,
            arena: false,
            encounter: None,
        },
        a: Box::new(CompareSide::default()),
        b: Box::new(CompareSide {
            guid: "Player-1-0B".to_string(),
            timeline: Timeline {
                bucket_ms: 1000,
                buckets: vec![1],
                marks: vec![Mark {
                    at_ms: AT,
                    kind,
                    label: "Power Infusion".to_string(),
                    spell_id: 10060,
                    dur_ms: 15_000,
                    src: "Player-1-0A".to_string(),
                }],
            },
            ..CompareSide::default()
        }),
        range: None,
        source: None,
        status: None,
    };
    let kinds = [
        MarkKind::TrinketUse,
        MarkKind::TrinketProc,
        MarkKind::Consumable,
        MarkKind::External,
        MarkKind::ActiveMitigation,
        MarkKind::Defensive,
        MarkKind::SupportBuff,
        MarkKind::Cooldown,
    ];
    assert_eq!(kinds.len(), 8, "a new kind needs a code AND a row here");
    for (i, kind) in kinds.into_iter().enumerate() {
        assert_eq!(kind.code(), i as u8, "{kind:?}");
        let msg = compare(kind);
        let back = roundtrip_daemon(&msg);
        assert_eq!(back, msg, "{kind:?}");
        let DaemonMsg::CompareSnapshot { b, .. } = back else {
            panic!("a compare snapshot")
        };
        assert_eq!(
            b.timeline.marks[0].src, "Player-1-0A",
            "the caster rides along"
        );
    }
    // The kind byte sits right after the mark's at_ms; the code past the
    // last variant is rejected.
    let mut frame = compare(MarkKind::Cooldown).encode();
    let pos = frame
        .windows(8)
        .position(|w| w == AT.to_le_bytes())
        .expect("the mark's at_ms")
        + 8;
    assert_eq!(frame[pos], MarkKind::Cooldown.code());
    frame[pos] = 8;
    let (tag, body) = wire::read_frame(&mut &frame[..]).expect("a whole frame");
    assert_eq!(DaemonMsg::decode(tag, &body), Err(DecodeError::BadTag(8)));
}
