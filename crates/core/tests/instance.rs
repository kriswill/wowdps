//! R10: instance visits and the per-visit Overall, against the committed
//! instance fixture — full replay semantics, scanner parity, lazy-load
//! parity, and checkpoint resumption with a visit in flight.

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, SegmentKind, View, meter_from_lines};

const INSTANCE_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/instance.txt");

fn replay() -> Meter {
    let text = std::fs::read_to_string(INSTANCE_FIXTURE).unwrap();
    meter_from_lines(text.lines())
}

fn amounts(seg: &wowdps_core::meter::Segment, view: View) -> Vec<(String, u64)> {
    seg.rows(view)
        .into_iter()
        .map(|r| (r.label, r.amount))
        .collect()
}

#[test]
fn visits_and_segment_tags_follow_the_zone_rules() {
    let meter = replay();

    let visits = meter.visits();
    assert_eq!(visits.len(), 2);
    assert_eq!(visits[0].name, "Algeth'ar Academy");
    assert_eq!(visits[0].key_level, Some(12));
    assert_eq!(visits[0].completed, Some(true), "the key timed");
    assert!(visits[0].end_ms.is_some(), "closed when Skyreach opened");
    assert_eq!(visits[0].display_name(), "Algeth'ar Academy +12");
    assert_eq!(visits[1].name, "Skyreach");
    assert_eq!(visits[1].difficulty, 23);
    assert_eq!(visits[1].key_level, None);
    assert!(visits[1].end_ms.is_none(), "still in progress at EOF");

    let tags: Vec<(SegmentKind, Option<u32>)> =
        meter.segments().iter().map(|s| (s.kind, s.visit)).collect();
    assert_eq!(
        tags,
        vec![
            (SegmentKind::Trash, Some(0)),     // Crawler pulls
            (SegmentKind::Encounter, Some(0)), // Vexamus
            (SegmentKind::Trash, Some(0)),     // Guardian
            (SegmentKind::Trash, None),        // city dummy
            (SegmentKind::Trash, Some(1)),     // Skyblade
            (SegmentKind::Encounter, Some(1)), // Ranjit
            (SegmentKind::Trash, None),        // city dummy while suspended
            (SegmentKind::Trash, Some(1)),     // Skyguard, after re-entry
        ]
    );
    // A zone change closes the open trash segment (R10 amendment to R4).
    assert!(meter.segments()[3].end_ms.is_some(), "city trash closed");
    assert!(meter.segments()[7].end_ms.is_none(), "trailing pull open");
}

#[test]
fn the_overall_accumulates_every_member_counter() {
    let meter = replay();

    let o0 = meter.overall(0).expect("visit 0 has members");
    assert_eq!(o0.kind, SegmentKind::Overall);
    assert_eq!(o0.name, "Algeth'ar Academy +12");
    assert_eq!(o0.success, Some(true));
    assert_eq!(o0.visit, Some(0));
    // Trash 100+150 then 50 for Ana, Vexamus 300/200: city combat excluded.
    assert_eq!(
        amounts(&o0, View::Damage),
        vec![
            ("Ana-Realm".to_string(), 600),
            ("Borin-Realm".to_string(), 200)
        ]
    );
    // Members' R7 durations: 20s of Crawler trash + 60s Vexamus + 0s tail.
    assert_eq!(o0.duration_ms(i64::MAX), 80_000);

    let o1 = meter.overall(1).expect("visit 1 has members");
    assert_eq!(o1.name, "Skyreach");
    assert!(o1.end_ms.is_none(), "live overall");
    assert_eq!(
        amounts(&o1, View::Damage),
        vec![
            ("Borin-Realm".to_string(), 500),
            ("Ana-Realm".to_string(), 140)
        ]
    );
    assert_eq!(o1.duration_ms(i64::MAX), 30_000);
}

#[test]
fn the_scanner_mirrors_visits_and_emits_overall_metas() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    // Segment kinds and visit tags agree with a full replay.
    let scanned: Vec<(SegmentKind, Option<u32>)> = idx
        .segments
        .iter()
        .map(|m| (m.kind, m.visit))
        .chain(idx.open.iter().map(|m| (m.kind, m.visit)))
        .collect();
    let replayed: Vec<(SegmentKind, Option<u32>)> =
        meter.segments().iter().map(|s| (s.kind, s.visit)).collect();
    assert_eq!(scanned, replayed);

    // The closed visit produced an Overall meta matching the replay.
    assert_eq!(idx.overalls.len(), 1);
    let m = &idx.overalls[0];
    let want = meter.overall(0).unwrap();
    assert_eq!(m.kind, SegmentKind::Overall);
    assert_eq!(m.name, want.name);
    assert_eq!(m.success, want.success);
    assert_eq!(m.visit, Some(0));
    assert_eq!(m.duration_ms, want.duration_ms(i64::MAX));

    // The in-progress visit surfaces as `open_visit`, duration included.
    let ov = idx.open_visit.as_ref().expect("Skyreach is in progress");
    assert_eq!(ov.name, "Skyreach");
    assert_eq!(ov.visit, Some(1));
    assert_eq!(ov.end_ms, None);
    assert_eq!(
        ov.duration_ms,
        meter.overall(1).unwrap().duration_ms(i64::MAX)
    );
}

#[test]
fn a_lazily_loaded_overall_matches_the_full_replay() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    for (meta, ordinal) in idx
        .overalls
        .iter()
        .chain(idx.open_visit.iter())
        .map(|m| (m, m.visit.unwrap()))
    {
        let lines = load_segment(std::path::Path::new(INSTANCE_FIXTURE), meta).unwrap();
        let lazy = meter_from_lines(lines.iter().map(String::as_str));
        let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
        let want = meter.overall(ordinal).unwrap();
        for view in [View::Damage, View::Healing, View::Deaths] {
            assert_eq!(
                amounts(&got, view),
                amounts(&want, view),
                "{:?} in {}",
                view,
                meta.name
            );
        }
        assert_eq!(
            got.duration_ms(i64::MAX),
            want.duration_ms(i64::MAX),
            "{}",
            meta.name
        );
        assert_eq!(got.name, want.name);
    }
}

#[test]
fn a_resumed_scan_matches_a_full_scan_mid_visit() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let full = scan(&mut &bytes[..]);
    let cuts: Vec<usize> = bytes
        .iter()
        .enumerate()
        .filter(|&(_, &b)| b == b'\n')
        .map(|(i, _)| i + 1)
        .chain([bytes.len() / 2, bytes.len()])
        .collect();
    for cut in cuts {
        let prefix = scan(&mut &bytes[..cut]);
        let state = prefix.checkpoint.clone();
        let off = state.offset as usize;
        let resumed = scan_from(&mut &bytes[off..], state);
        assert_eq!(resumed.segments, full.segments, "cut at {cut}");
        assert_eq!(resumed.overalls, full.overalls, "cut at {cut}");
        assert_eq!(resumed.open_visit, full.open_visit, "cut at {cut}");
        assert_eq!(resumed.open, full.open, "cut at {cut}");
        assert_eq!(resumed.checkpoint, full.checkpoint, "cut at {cut}");
    }
}
