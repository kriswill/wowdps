//! Every enum code the wire carries roundtrips — views, classes, specs,
//! client kinds, load errors, overlay states — so a variant added to the
//! model without a code (or with a colliding one) fails here, not in a
//! frontend that suddenly decodes the wrong thing.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use wowdps_model::{Class, Row, SegmentInfo, SegmentKind, Spec, View};
use wowdps_proto::wire;
use wowdps_proto::{
    ClientKind, ClientMsg, DaemonMsg, HistoryStatus, LoadError, OverlayState, SegmentRef,
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
