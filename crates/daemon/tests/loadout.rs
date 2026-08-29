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
fn a_segment_never_answers_with_a_loadout_first_seen_after_it() {
    // "As known to THIS segment": a COMBATANT_INFO that first fires in
    // segment B must not leak into a query about the earlier, still-warm
    // segment A — the warm answer has to match what a lazy replay of A
    // would say after a restart (None).
    let mut mock = MockDaemon::fixture();
    const NEW: &str = "Player-9-NEWGUY";
    let dmg = |ts: &str| {
        format!(
            "{ts}  SPELL_DAMAGE,{NEW},\"New-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,\
             116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil"
        )
    };
    // Segment A: combat with no COMBATANT_INFO for NEW.
    let out = mock.feed(vec![dmg("7/27/2026 23:00:00.000-4")]);
    let id_a = out
        .iter()
        .find_map(|m| match m {
            DaemonMsg::SegmentOpened { id } => Some(*id),
            _ => None,
        })
        .expect("segment A opens");
    // >60s lull: segment B opens, and only B sees NEW's info.
    mock.feed(vec![
        dmg("7/27/2026 23:02:00.000-4"),
        format!(
            "7/27/2026 23:02:01.000-4  COMBATANT_INFO,{NEW},1,2129,217,26548,664,0,0,0,0,968,\
             968,968,221,0,668,668,668,0,1062,73,73,73,2361,70,[(81523,102493,1)],(0,0),[],[]"
        ),
    ]);
    let out = ask(&mut mock, 5, SegmentRef::Id(id_a), NEW);
    assert!(
        matches!(
            &out[..],
            [DaemonMsg::Loadout {
                req_id: 5,
                loadout: None,
                ..
            }]
        ),
        "segment A must not see B's build: {out:?}"
    );
    // The live segment (B) does know it.
    let out = ask(&mut mock, 6, SegmentRef::Live, NEW);
    let [
        DaemonMsg::Loadout {
            req_id: 6,
            loadout: Some(l),
            ..
        },
    ] = &out[..]
    else {
        panic!("expected B's build live, got {out:?}");
    };
    assert_eq!(l.spec_id, Some(70));
    assert_eq!(l.talents.len(), 1);
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
