//! v19: the `GetLoadout` one-shot over the real engine + fixture — live
//! answers, lazily loaded history (parity with the full replay), and the
//! defined-to-never-error unknowns.

use wowdps_daemon::mock::MockDaemon;
use wowdps_proto::{ClientMsg, DaemonMsg, SegmentRef};

const P1: &str = "Player-1168-0A1B2C01";

fn ask(mock: &mut MockDaemon, req_id: u32, segment: SegmentRef, guid: &str) -> Vec<DaemonMsg> {
    mock.handle(ClientMsg::GetLoadout {
        req_id,
        segment,
        guid: guid.to_string(),
    })
}

#[test]
fn live_answers_with_the_fixture_players_logged_build() {
    let mut mock = MockDaemon::fixture();
    let out = ask(&mut mock, 1, SegmentRef::Live, P1);
    let [
        DaemonMsg::Loadout {
            req_id: 1,
            guid,
            loadout: Some(l),
        },
    ] = &out[..]
    else {
        panic!("expected one Loadout reply, got {out:?}");
    };
    assert_eq!(guid, P1);
    // Fixture line 15: Arms (71), three picks (one rank-0), two gear items.
    assert_eq!(l.spec_id, Some(71));
    assert_eq!(l.talents.len(), 3);
    assert_eq!((l.talents[2].node_id, l.talents[2].rank), (91026, 0));
    assert_eq!(l.gear.len(), 2);
    assert_eq!(l.gear[0].item_id, 212446);
    assert_eq!(l.gear[1].gems, vec![213743]);
}

#[test]
fn a_historical_segment_loads_lazily_and_answers_identically() {
    let mut mock = MockDaemon::fixture();
    // Resolve a closed historical segment's id off the list.
    let out = mock.handle(ClientMsg::Watch(wowdps_proto::Cursor::List));
    let ids: Vec<_> = out
        .iter()
        .find_map(|m| match m {
            DaemonMsg::SegmentList { entries, .. } => {
                Some(entries.iter().map(|e| e.id).collect::<Vec<_>>())
            }
            _ => None,
        })
        .expect("a segment list");
    assert!(!ids.is_empty());
    let first = ids[0];
    let out = ask(&mut mock, 2, SegmentRef::Id(first), P1);
    let [
        DaemonMsg::Loadout {
            req_id: 2,
            loadout: Some(l),
            ..
        },
    ] = &out[..]
    else {
        panic!("expected a loaded Loadout reply, got {out:?}");
    };
    // The lazily loaded slice replays the same seeds: identical answer.
    assert_eq!(l.spec_id, Some(71));
    assert_eq!(l.talents.len(), 3);
    assert_eq!(l.gear.len(), 2);
}

#[test]
fn unknown_guid_and_dead_id_answer_none_never_error() {
    let mut mock = MockDaemon::fixture();
    let out = ask(&mut mock, 3, SegmentRef::Live, "Player-9999-DEADBEEF");
    assert!(
        matches!(
            &out[..],
            [DaemonMsg::Loadout {
                req_id: 3,
                loadout: None,
                ..
            }]
        ),
        "unknown guid answers None: {out:?}"
    );
    let out = ask(
        &mut mock,
        4,
        SegmentRef::Id(wowdps_model::SegmentId(u64::MAX)),
        P1,
    );
    assert!(
        matches!(
            &out[..],
            [DaemonMsg::Loadout {
                req_id: 4,
                loadout: None,
                ..
            }]
        ),
        "a dead id answers None: {out:?}"
    );
}
