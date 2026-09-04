//! R17 through the daemon (`PROTO_VERSION` 21): a Taken watch answers rate
//! rows and — when drilled — a `Breakdown` carrying the player's mitigation
//! record; the history store's rows tier carries the Taken rows (its seventh
//! view) so `stored_fight(Taken)` equals the live meter's rows. Numbers are
//! `crates/core/fixtures/taken.expected.md`'s.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::Path;

use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{MissKind, Row, SegmentKind, View};
use wowdps_proto::history::VIEW_KEYS;
use wowdps_proto::{Breakdown, ClientMsg, Cursor, DaemonMsg, SegmentRef};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const TAKEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");

const DURGAN: &str = "Player-1168-0A1B2C11";
const ZENLI: &str = "Player-1168-0A1B2C12";
const PYRALIS: &str = "Player-1168-0A1B2C13";

/// The segment ids the mock lists, in list order.
fn ids(mock: &mut MockDaemon) -> Vec<wowdps_model::SegmentId> {
    let out = mock.handle(ClientMsg::Watch(Cursor::List));
    let list = out
        .iter()
        .find_map(|m| match m {
            DaemonMsg::SegmentList { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .expect("a list answers the list cursor");
    list.iter().map(|e| e.id).collect()
}

/// Watch `view` on `id` (optionally drilled) and return the settled
/// snapshot's rows and breakdown.
fn watch(
    mock: &mut MockDaemon,
    id: wowdps_model::SegmentId,
    view: View,
    drill: Option<&str>,
) -> (wowdps_model::SegmentInfo, Vec<Row>, Option<Breakdown>) {
    let out = mock.handle(ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Id(id),
        view,
        top_n: None,
        drill: drill.map(str::to_string),
        spell: None,
    }));
    out.into_iter()
        .rev()
        .find_map(|m| match m {
            DaemonMsg::Snapshot {
                view: v,
                info,
                rows,
                breakdown,
                ..
            } if v == view => Some((info, rows, breakdown)),
            _ => None,
        })
        .expect("a snapshot answers the segment cursor")
}

/// The fixture's boss encounter, as the mock lists it.
fn boss(mock: &mut MockDaemon) -> wowdps_model::SegmentId {
    ids(mock)
        .into_iter()
        .find(|id| {
            let (info, _, _) = watch(mock, *id, View::Damage, None);
            info.kind == SegmentKind::Encounter && info.name == "Taken Test Boss"
        })
        .expect("taken.txt lists its boss")
}

fn row<'a>(rows: &'a [Row], key: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("{key} has a row: {rows:?}"))
}

#[test]
fn a_taken_watch_answers_rate_rows_and_a_drill_carries_the_mitigation_record() {
    let mut mock = MockDaemon::fixture_at(Path::new(TAKEN));
    let boss = boss(&mut mock);

    // Undrilled: three rows (pets folded), DTPS over the 60 s kill, `extra`
    // = absorbed, no breakdown.
    let (info, rows, breakdown) = watch(&mut mock, boss, View::Taken, None);
    assert_eq!(info.duration_ms, 60_000);
    assert!(breakdown.is_none(), "no drill, no breakdown");
    assert_eq!(rows.len(), 3, "pets fold onto owners: {rows:?}");
    let durgan = row(&rows, DURGAN);
    assert_eq!((durgan.amount, durgan.extra), (84_000, 12_000));
    assert!(
        (durgan.per_sec - 1400.0).abs() < 1e-9,
        "DTPS: {}",
        durgan.per_sec
    );
    let zenli = row(&rows, ZENLI);
    assert_eq!((zenli.amount, zenli.extra), (70_200, 25_000));
    assert!(
        (zenli.per_sec - 1170.0).abs() < 1e-9,
        "DTPS: {}",
        zenli.per_sec
    );
    let pyralis = row(&rows, PYRALIS);
    assert_eq!((pyralis.amount, pyralis.extra), (52_000, 5_000));
    assert!(rows.iter().all(|r| r.per_sec > 0.0), "Taken is a rate view");
    assert_eq!(rows[0].key, DURGAN, "sorted by amount taken");

    // Drilled on the tank: by-ability / by-attacker rows, no timeline (the
    // coarse taken series is step 4), and the mitigation record.
    let (_, drilled, breakdown) = watch(&mut mock, boss, View::Taken, Some(DURGAN));
    assert_eq!(drilled, rows, "the drill leaves the meter rows alone");
    let b = breakdown.expect("a drilled Taken watch answers a breakdown");
    assert!(b.timeline.is_none(), "no taken timeline in v21");
    assert!(b.spell_timeline.is_none());
    assert!(!b.by_spell.is_empty());
    assert!(!b.by_target.is_empty());
    let boss_row = row(&b.by_target, "Taken Test Boss");
    assert_eq!(boss_row.amount, 84_000, "attackers keyed by name");
    let m = b
        .mitigation
        .expect("Taken drill carries the mitigation record");
    assert_eq!(
        (m.absorbed, m.blocked, m.absorbed_full, m.blocked_full),
        (12_000, 18_000, 0, 55_000)
    );
    assert_eq!((m.stagger, m.stagger_ticked), (0, 0));
    assert_eq!(m.misses(), 5);
    for kind in [MissKind::Block, MissKind::Parry, MissKind::Dodge] {
        assert_eq!(m.misses[kind.index()], 1, "{kind:?}");
    }
    assert_eq!(m.misses[MissKind::Miss.index()], 2);
    assert_eq!(m.mitigated(), 85_000);

    // The monk: stagger reported, never added; the ticks excluded.
    let (_, _, breakdown) = watch(&mut mock, boss, View::Taken, Some(ZENLI));
    let m = breakdown
        .and_then(|b| b.mitigation)
        .expect("Zenlí's record");
    assert_eq!(
        (m.absorbed, m.stagger, m.stagger_ticked),
        (25_000, 25_000, 10_000)
    );
    assert_eq!((m.absorbed_full, m.blocked, m.blocked_full), (3_000, 0, 0));
    assert_eq!(m.misses(), 1);
    assert_eq!(m.mitigated(), 28_000);

    // The mage and the pet: the pre-summon hit folds, "Environment" is an
    // attacker, and the add's EVADE of the mage's own cast is nobody's miss.
    let (_, _, breakdown) = watch(&mut mock, boss, View::Taken, Some(PYRALIS));
    let b = breakdown.expect("Pyralis' breakdown");
    assert!(
        b.by_target.iter().any(|r| r.key == "Environment"),
        "{:?}",
        b.by_target
    );
    let m = b.mitigation.expect("Pyralis' record");
    assert_eq!((m.absorbed, m.absorbed_full), (5_000, 21_000));
    assert_eq!(m.misses(), 5);
    assert_eq!(
        m.misses[MissKind::Evade.index()],
        0,
        "the add evaded, not F"
    );
    for kind in [
        MissKind::Immune,
        MissKind::Absorb,
        MissKind::Deflect,
        MissKind::Reflect,
        MissKind::Resist,
    ] {
        assert_eq!(m.misses[kind.index()], 1, "{kind:?}");
    }

    // Any other drilled view keeps `mitigation` absent — present iff Taken.
    for view in [View::Damage, View::Healing, View::Deaths, View::Interrupts] {
        let (_, _, breakdown) = watch(&mut mock, boss, view, Some(DURGAN));
        let b = breakdown.expect("a drill answers a breakdown");
        assert!(b.mitigation.is_none(), "{view:?} carries no mitigation");
    }
    let (_, _, breakdown) = watch(&mut mock, boss, View::Damage, Some(DURGAN));
    assert!(
        breakdown.unwrap().timeline.is_some(),
        "Damage keeps its curve"
    );
}

#[test]
fn an_unknown_drill_under_taken_has_no_record() {
    let mut mock = MockDaemon::fixture_at(Path::new(TAKEN));
    let boss = boss(&mut mock);
    let (_, _, breakdown) = watch(&mut mock, boss, View::Taken, Some("Player-0-nobody"));
    let b = breakdown.expect("a drill always answers a breakdown");
    assert!(b.by_spell.is_empty() && b.by_target.is_empty());
    assert!(b.mitigation.is_none(), "nobody took nothing");
}

/// Replay a whole log through an engine the way the tail thread would,
/// collecting every `Closed` fight.
fn closed_fights(path: &Path) -> Vec<ClosedFight> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.to_path_buf()), &mut events);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    engine.on_tail(TailEvent::Lines(lines), &mut events);
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Closed(id) => engine.take_closed(*id),
            EngineEvent::Opened(_) => None,
        })
        .collect()
}

#[test]
fn the_rows_tier_carries_taken_as_its_seventh_view_for_every_fight() {
    assert_eq!(VIEW_KEYS[View::Taken.index()], (View::Taken, "taken"));
    assert_eq!(View::Taken.index(), 6);

    for log in [SAMPLE, TAKEN] {
        let path = Path::new(log);
        let facts = LogFacts::read(path);
        let mut store = Store::open(MemBackend::new(), Retention::default());
        let fights = closed_fights(path);
        assert!(!fights.is_empty(), "{log} closes fights");
        let mut any_taken = false;
        let mut stored = 0;
        for fight in &fights {
            // The store declines what it never keeps (out-of-instance
            // trash); every kept fight carries the seventh view.
            let Some(id) = store.store(fight, facts) else {
                continue;
            };
            stored += 1;
            let live = fight.segment.rows(View::Taken);
            any_taken |= !live.is_empty();

            // Read back through the JSON codec: the seventh slot IS the
            // live meter's Taken rows.
            let rows = store.rows(&id).expect("rows tier");
            assert_eq!(rows.views[6], live, "{log} {id}: views[6]");
            assert_eq!(rows.rows(View::Taken), &live[..]);
            let json = rows.to_json().to_line();
            assert!(json.contains("\"taken\":["), "{json}");

            // `stored_fight(Taken)` serves those rows and, for now, no
            // drill (step 2b puts the drills and the record on the tier).
            let drill = live.first().map(|r| r.key.clone());
            let sf = store
                .stored_fight(&id, View::Taken, drill.as_deref())
                .expect("the card exists");
            assert_eq!(sf.rows, live);
            assert!(sf.breakdown.is_none(), "no Taken drill until step 2b");
            assert!(sf.tier >= 2, "answered from the rows tier: {}", sf.tier);
            // The record is on the live segment, not the store.
            if let Some(guid) = &drill {
                assert!(fight.segment.mitigation(guid).is_some());
            }
        }
        assert!(stored >= 1, "{log}: {stored} fights stored");
        assert!(any_taken, "{log} has friendly-destination damage");
    }
}

#[test]
fn the_stored_taken_rows_equal_the_live_snapshot_through_the_mock() {
    let mut mock = MockDaemon::fixture_at(Path::new(TAKEN)).with_history();
    let boss = boss(&mut mock);
    let (_, live, _) = watch(&mut mock, boss, View::Taken, Some(DURGAN));

    let cards: Vec<_> = mock
        .history()
        .cards()
        .iter()
        .filter(|c| c.name == "Taken Test Boss")
        .cloned()
        .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    let out = mock.handle(ClientMsg::GetFight {
        req_id: 7,
        fight_id: cards[0].id.clone(),
        view: View::Taken,
        drill: Some(DURGAN.to_string()),
        boss: None,
    });
    let [
        DaemonMsg::Fight {
            req_id: 7,
            fight: Some(f),
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(f.rows, live, "stored Taken rows are the live rows");
    assert!(f.breakdown.is_none(), "no Taken drill until step 2b");
}
