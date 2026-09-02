//! The engine over indexed history, driven the way the tail thread and the
//! hub drive it: a closed visit's Overall is answered with a loading
//! placeholder, then — once its slice is installed — with merged rows,
//! comparisons and loadouts; tail status events land in the snapshot
//! footer; the trash can tombstones indexed out-of-instance trash.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use wowdps_core::index::{self, load_segment};
use wowdps_core::meter::meter_from_lines;
use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Built, Engine, LoadoutBuilt};
use wowdps_model::{ListRow, SegmentId, SegmentKind, View};
use wowdps_proto::{DaemonMsg, SegmentRef, is_loading_status};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const INSTANCE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/instance.txt");
const P1: &str = "Player-1168-0A1B2C01";

/// A log the way the tail thread delivers it on open: scan, `Switched`,
/// `Index`, the seed lines, the tail from `live_offset`, `CaughtUp`.
fn boot(log: &str) -> (Engine, PathBuf) {
    let path = PathBuf::from(log);
    let bytes = std::fs::read(&path).unwrap();
    let idx = index::scan(&mut &bytes[..]);
    let live = idx.live_offset as usize;
    let seed_ranges = match idx.open.as_ref() {
        Some(open) => open.seeds.clone(),
        None => idx.checkpoint.seeds.clone(),
    };
    let seeds: Vec<String> = seed_ranges
        .iter()
        .map(|&(s, e)| {
            String::from_utf8_lossy(&bytes[s as usize..e as usize])
                .trim_end()
                .to_string()
        })
        .collect();
    let tail: Vec<String> = String::from_utf8_lossy(&bytes[live..])
        .lines()
        .map(str::to_string)
        .collect();

    let mut e = Engine::new();
    let mut events = Vec::new();
    e.on_tail(TailEvent::Switched(path.clone()), &mut events);
    e.on_tail(
        TailEvent::Index {
            index: Box::new(idx),
            file_age_ms: Some(0),
        },
        &mut events,
    );
    if !seeds.is_empty() {
        e.on_tail(TailEvent::Lines(seeds), &mut events);
    }
    e.on_tail(TailEvent::Lines(tail), &mut events);
    e.on_tail(TailEvent::CaughtUp, &mut events);
    assert!(events.is_empty(), "backlog never announces");
    (e, path)
}

fn entries(e: &Engine) -> Vec<(SegmentId, ListRow)> {
    match e.build_list(false) {
        DaemonMsg::SegmentList { entries, .. } => {
            entries.into_iter().map(|x| (x.id, x.row)).collect()
        }
        other => panic!("{other:?}"),
    }
}

/// A warm segment answers at once.
fn ready(built: Built) -> DaemonMsg {
    match built {
        Built::Ready(msg) => *msg,
        Built::Loading(_, id, _) => panic!("{id:?} is still cold"),
        Built::Failed(id, err) => panic!("{id:?} failed: {err:?}"),
    }
}

/// Serve a cold segment the way the loader pool would.
fn install(e: &mut Engine, path: &Path, built: Built) {
    let Built::Loading(placeholder, id, meta) = built else {
        panic!("expected a cold segment");
    };
    if let DaemonMsg::Snapshot { status, rows, .. } = *placeholder {
        assert!(status.as_deref().is_some_and(is_loading_status));
        assert!(rows.is_empty(), "placeholders carry no rows");
    }
    let lines = load_segment(path, &meta).unwrap();
    e.install_loaded(id, meter_from_lines(lines.iter().map(String::as_str)));
}

/// R10: the instance fixture's completed key is a CLOSED visit — its
/// Overall comes from the scan and is replayed from the visit's byte range
/// on first use, then serves meter, comparison and loadout warm.
#[test]
fn a_closed_visits_overall_loads_lazily_for_meter_compare_and_loadout() {
    let (mut e, path) = boot(INSTANCE);
    let list = entries(&e);
    let (overall, row) = list
        .iter()
        .find(|(_, r)| r.kind == SegmentKind::Overall && !r.live && r.pars_ms.is_some())
        .cloned()
        .expect("the completed key's Σ row");
    assert_eq!(row.success, Some(true), "timed");
    let sref = SegmentRef::Id(overall);

    // Cold: a placeholder, and the one-shot parks too.
    assert!(matches!(e.loadout(sref, "Player-1-A"), LoadoutBuilt::Loading(i, _) if i == overall));
    let cold = e.build_segment(sref, View::Damage, Some(1), None, None);
    install(&mut e, &path, cold);
    assert_eq!(e.resident(), 1);

    // Warm: merged rows, capped by top_n but counted in full.
    let msg = ready(e.build_segment(sref, View::Damage, Some(1), None, None));
    let DaemonMsg::Snapshot {
        id,
        info,
        rows,
        total_rows,
        status,
        ..
    } = msg
    else {
        panic!("a snapshot");
    };
    assert_eq!(id, Some(overall));
    assert_eq!(info.kind, SegmentKind::Overall);
    assert_eq!(info.name, row.name);
    assert_eq!(info.pars_ms, row.pars_ms);
    assert_eq!(info.instance, row.instance);
    assert!(!info.live);
    assert_eq!(rows.len(), 1, "top_n = 1");
    assert!(total_rows >= 1);
    assert!(status.is_none());

    // A healing drill carries the healing curve (v14), no ability curve.
    let msg = ready(e.build_segment(sref, View::Healing, None, Some("Player-1-A"), None));
    let DaemonMsg::Snapshot { breakdown, .. } = msg else {
        panic!("a snapshot");
    };
    let bd = breakdown.expect("drilled");
    assert!(bd.timeline.is_some(), "healing has a curve");
    assert!(bd.spell_timeline.is_none(), "no ability drill");

    // The comparison over the merged visit.
    let msg = ready(e.build_compare(sref, "Player-1-A", "Player-1-B", None, None));
    let DaemonMsg::CompareSnapshot { a, b, info, .. } = msg else {
        panic!("a comparison");
    };
    assert_eq!(info.kind, SegmentKind::Overall);
    assert_eq!(
        (a.guid.as_str(), b.guid.as_str()),
        ("Player-1-A", "Player-1-B")
    );

    // The loadout: answered warm — None, this log has no COMBATANT_INFO.
    assert!(matches!(
        e.loadout(sref, "Player-1-A"),
        LoadoutBuilt::Ready(None)
    ));

    // A member of the closed visit is its own slice; the live tail's
    // segments need no load at all.
    let (member, _) = list
        .iter()
        .find(|(_, r)| r.kind == SegmentKind::Encounter && r.instance == row.instance)
        .cloned()
        .expect("the key's boss");
    let cold = e.build_segment(SegmentRef::Id(member), View::Damage, None, None, None);
    install(&mut e, &path, cold);
    assert_eq!(e.resident(), 2);
    let (live, live_row) = list.last().cloned().unwrap();
    assert!(live_row.live);
    ready(e.build_segment(SegmentRef::Id(live), View::Damage, None, None, None));
    assert_eq!(e.resident(), 2, "live segments are never loaded");
}

#[test]
fn the_live_tail_answers_at_once_with_the_logged_build() {
    let (mut e, path) = boot(SAMPLE);
    let list = entries(&e);
    assert_eq!(list.len(), 5, "four segments plus the visit's Σ");
    // The fixture ends with every segment closed: Live is the newest
    // indexed segment, served lazily like any other history.
    let cold = e.build_segment(SegmentRef::Live, View::Damage, None, None, None);
    install(&mut e, &path, cold);
    let msg = ready(e.build_segment(SegmentRef::Live, View::Damage, None, None, None));
    let DaemonMsg::Snapshot { info, rows, .. } = msg else {
        panic!("a snapshot");
    };
    assert_eq!(info.name, "Verkath the Hollow");
    assert_eq!(rows.len(), 3);
    match e.loadout(SegmentRef::Live, P1) {
        LoadoutBuilt::Ready(Some(l)) => assert_eq!(l.spec_id, Some(71)),
        _ => panic!("the live segment knows the fixture player's build"),
    }
    // The visit is still open at the end of the log: its Σ merges the
    // scanned prefix (loaded once) with whatever the live meter holds.
    let (overall, row) = list[0].clone();
    assert_eq!(row.kind, SegmentKind::Overall);
    assert!(row.live);
    let cold = e.build_segment(SegmentRef::Id(overall), View::Damage, None, None, None);
    install(&mut e, &path, cold);
    let msg = ready(e.build_segment(SegmentRef::Id(overall), View::Damage, None, None, None));
    let DaemonMsg::Snapshot { info, rows, .. } = msg else {
        panic!("a snapshot");
    };
    assert_eq!(info.kind, SegmentKind::Overall);
    assert_eq!(info.name, "Sepulcher of the Ashen Vow");
    assert_eq!(rows.len(), 3);
    match e.loadout(SegmentRef::Id(overall), P1) {
        LoadoutBuilt::Ready(Some(l)) => assert_eq!(l.spec_id, Some(71)),
        _ => panic!("the live visit's Σ answers from the meter"),
    }
    assert_eq!(e.resident(), 2);
}

#[test]
fn an_empty_engine_answers_loadouts_with_none() {
    let mut e = Engine::default();
    assert!(matches!(
        e.loadout(SegmentRef::Live, P1),
        LoadoutBuilt::Ready(None)
    ));
    assert!(matches!(
        e.loadout(SegmentRef::Id(SegmentId(5)), P1),
        LoadoutBuilt::Ready(None)
    ));
    assert_eq!(e.segment_count(), 0);
}

#[test]
fn tail_status_events_reach_the_snapshot_footer() {
    let (mut e, path) = boot(SAMPLE);
    let mut events = Vec::new();
    e.on_tail(
        TailEvent::Error("log.txt: permission denied".to_string()),
        &mut events,
    );
    let cold = e.build_segment(SegmentRef::Live, View::Damage, None, None, None);
    install(&mut e, &path, cold);
    let msg = ready(e.build_segment(SegmentRef::Live, View::Damage, None, None, None));
    let DaemonMsg::Snapshot { status, source, .. } = msg else {
        panic!("a snapshot");
    };
    assert_eq!(status.as_deref(), Some("log.txt: permission denied"));
    assert_eq!(source.as_deref(), Some("sample.txt"));

    e.on_tail(TailEvent::Waiting, &mut events);
    let DaemonMsg::SegmentList { source, .. } = e.build_list(false) else {
        panic!("a list");
    };
    assert_eq!(source, None, "waiting for a log means no source to name");
}

/// R11 over indexed history: closed, out-of-instance trash from the scan
/// is tombstoned; the fixture's visit members (trash included) survive.
#[test]
fn discard_trash_tombstones_indexed_out_of_instance_trash_only() {
    let hit = |min: u32| {
        format!(
            "7/27/2026 21:{min:02}:00.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n"
        )
    };
    let log = format!("{}{}{}", hit(0), hit(10), hit(20));
    let idx = index::scan(&mut log.as_bytes());
    assert_eq!(idx.segments.len(), 2, "two closed trash pulls, one open");
    let mut e = Engine::new();
    let mut events = Vec::new();
    e.on_tail(
        TailEvent::Switched(PathBuf::from("/tmp/x.txt")),
        &mut events,
    );
    e.on_tail(
        TailEvent::Index {
            index: Box::new(idx),
            file_age_ms: None,
        },
        &mut events,
    );
    assert_eq!(e.list_rows().len(), 2);
    e.discard_trash();
    assert!(e.list_rows().is_empty(), "both were out-of-instance trash");

    let (mut e, _) = boot(SAMPLE);
    let before = e.list_rows().len();
    e.discard_trash();
    assert_eq!(
        e.list_rows().len(),
        before,
        "visit members are never discarded"
    );
}

/// R10 mid-visit attach: the daemon starts while a raid visit is under way
/// and a pull is open. The visit's Σ merges the scanned prefix (loaded once)
/// INTO the live members, so the merged rows exceed either half alone.
#[test]
fn a_live_visits_overall_absorbs_its_scanned_prefix() {
    let bytes = std::fs::read(SAMPLE).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let cut = text
        .rfind("ENCOUNTER_END")
        .expect("the fixture has encounters");
    let dir = std::env::temp_dir().join(format!("wowdps-engine-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("live.txt");
    std::fs::write(&path, &bytes[..cut]).unwrap();

    let (mut e, path) = boot(path.to_str().unwrap());
    let list = entries(&e);
    let (overall, row) = list[0].clone();
    assert_eq!(row.kind, SegmentKind::Overall);
    assert!(row.live, "the visit is open");
    let (live, live_row) = list.last().cloned().unwrap();
    assert!(live_row.live, "the last pull is open");
    let live_msg = ready(e.build_segment(SegmentRef::Id(live), View::Damage, None, None, None));
    let DaemonMsg::Snapshot {
        rows: live_rows, ..
    } = live_msg
    else {
        panic!("a snapshot");
    };
    let live_top = live_rows.iter().map(|r| r.amount).max().unwrap_or(0);
    assert!(live_top > 0, "the open pull has damage");

    let cold = e.build_segment(SegmentRef::Id(overall), View::Damage, None, None, None);
    install(&mut e, &path, cold);
    let msg = ready(e.build_segment(SegmentRef::Id(overall), View::Damage, None, None, None));
    let DaemonMsg::Snapshot { info, rows, .. } = msg else {
        panic!("a snapshot");
    };
    assert_eq!(info.kind, SegmentKind::Overall);
    assert!(info.live);
    let merged_top = rows.iter().map(|r| r.amount).max().unwrap_or(0);
    // Thraxx's first kill alone is 185 370 (golden total); with the open
    // pull absorbed the merged row is strictly more than either half.
    assert!(merged_top > live_top, "{merged_top} vs live {live_top}");
    assert!(merged_top > 185_370, "{merged_top}");
    let _ = std::fs::remove_dir_all(&dir);
}
