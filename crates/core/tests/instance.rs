//! R10: instance visits and the per-visit Overall, against the committed
//! instance fixture — full replay semantics, scanner parity, lazy-load
//! parity, and checkpoint resumption with a visit in flight.

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, SegmentKind, View, meter_from_lines};

const INSTANCE_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/instance.txt");

fn replay() -> Meter {
    let text = std::fs::read_to_string(INSTANCE_FIXTURE);
    assert!(text.is_ok(), "{INSTANCE_FIXTURE}: unreadable fixture");
    let text = text.unwrap_or_default();
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
    assert_eq!(visits.len(), 3);
    assert_eq!(visits[0].name, "Algeth'ar Academy");
    assert_eq!(visits[0].key_level, None, "the zone-in visit is pre-key");
    assert_eq!(visits[0].completed, None, "the zeroed reset END is ignored");
    assert!(visits[0].end_ms.is_some(), "closed when the key started");
    assert_eq!(visits[1].name, "Algeth'ar Academy");
    assert_eq!(visits[1].key_level, Some(12));
    assert_eq!(visits[1].completed, Some(true), "the key timed");
    assert!(visits[1].end_ms.is_some(), "closed when Skyreach opened");
    assert_eq!(visits[1].display_name(), "Algeth'ar Academy +12");
    assert_eq!(
        visits[1].start_ms,
        visits[0].end_ms.unwrap(),
        "the key's clock starts at CHALLENGE_MODE_START, not at the door"
    );
    // Bosses carry ENCOUNTER_START identity; trash and arena segments never do.
    let encs: Vec<_> = meter
        .segments()
        .iter()
        .filter_map(|s| s.encounter)
        .map(|e| (e.id, e.difficulty, e.group_size))
        .collect();
    assert_eq!(encs, vec![(2562, 8, 5), (1698, 23, 5)]);
    assert_eq!(
        meter.segments()[0].build,
        (12, 0, 0),
        "seeded from the version line"
    );
    assert_eq!(visits[2].name, "Skyreach");
    assert_eq!(visits[2].difficulty, 23);
    assert_eq!(visits[2].key_level, None);
    assert!(visits[2].end_ms.is_none(), "still in progress at EOF");

    let tags: Vec<(SegmentKind, Option<u32>)> =
        meter.segments().iter().map(|s| (s.kind, s.visit)).collect();
    assert_eq!(
        tags,
        vec![
            (SegmentKind::Trash, Some(0)),     // pre-key Crawler poke
            (SegmentKind::Trash, Some(1)),     // Crawler, once the key ran
            (SegmentKind::Encounter, Some(1)), // Vexamus
            (SegmentKind::Trash, Some(1)),     // Guardian
            (SegmentKind::Trash, None),        // city dummy
            (SegmentKind::Trash, Some(2)),     // Skyblade
            (SegmentKind::Encounter, Some(2)), // Ranjit
            (SegmentKind::Trash, None),        // city dummy while suspended
            (SegmentKind::Trash, Some(2)),     // Skyguard, after re-entry
        ]
    );
    // A zone change closes the open trash segment (R10 amendment to R4).
    assert!(meter.segments()[4].end_ms.is_some(), "city trash closed");
    assert!(meter.segments()[8].end_ms.is_none(), "trailing pull open");
}

#[test]
fn the_overall_accumulates_every_member_counter() {
    let meter = replay();

    // The zone-in visit keeps only the pre-key poke — the keyed run does
    // not inherit it, in clock or in counters.
    let o0 = meter.overall(0).expect("the zone-in visit has a member");
    assert_eq!(o0.kind, SegmentKind::Overall);
    assert_eq!(o0.name, "Algeth'ar Academy");
    assert_eq!(o0.success, None);
    assert_eq!(o0.visit, Some(0));
    assert_eq!(
        amounts(&o0, View::Damage),
        vec![("Ana-Realm".to_string(), 100)]
    );
    assert_eq!(
        o0.duration_ms(i64::MAX),
        0,
        "a single-event trash has no span"
    );

    let o1 = meter.overall(1).expect("the keyed visit has members");
    assert_eq!(o1.name, "Algeth'ar Academy +12");
    assert_eq!(o1.success, Some(true));
    assert_eq!(o1.visit, Some(1));
    // Trash 150 then 50 for Ana, Vexamus 300/200: the pre-key poke and
    // city combat are both excluded.
    assert_eq!(
        amounts(&o1, View::Damage),
        vec![
            ("Ana-Realm".to_string(), 500),
            ("Borin-Realm".to_string(), 200)
        ]
    );
    // The keyed clock is CHALLENGE_MODE_END's official totalMs, not the
    // member combat sum (0s Crawler trash + 60s Vexamus + 0s tail).
    assert_eq!(o1.duration_ms(i64::MAX), 900_000);

    let o2 = meter.overall(2).expect("visit 2 has members");
    assert_eq!(o2.name, "Skyreach");
    assert!(o2.end_ms.is_none(), "live overall");
    assert_eq!(
        amounts(&o2, View::Damage),
        vec![
            ("Borin-Realm".to_string(), 500),
            ("Ana-Realm".to_string(), 180)
        ]
    );
    // 0s Skyblade + 30s Ranjit + the open Skyguard pull cut at its last
    // combat event (15s).
    assert_eq!(o2.duration_ms(i64::MAX), 45_000);
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

    // Encounter identity (id / difficulty / group size) mirrors too.
    let scanned_enc: Vec<_> = idx
        .segments
        .iter()
        .chain(idx.open.iter())
        .map(|m| m.encounter)
        .collect();
    let replayed_enc: Vec<_> = meter.segments().iter().map(|s| s.encounter).collect();
    assert_eq!(scanned_enc, replayed_enc);

    // Both closed visits produced Overall metas matching the replay.
    assert_eq!(idx.overalls.len(), 2);
    for (ord, m) in idx.overalls.iter().enumerate() {
        let want = meter.overall(ord as u32).unwrap();
        assert_eq!(m.kind, SegmentKind::Overall);
        assert_eq!(m.name, want.name);
        assert_eq!(m.success, want.success);
        assert_eq!(m.visit, Some(ord as u32));
        assert_eq!(m.duration_ms, want.duration_ms(i64::MAX));
    }

    // The in-progress visit surfaces as `open_visit`: the prefix the live
    // tail cannot see. The open Skyguard pull is the live meter's — it is
    // excluded from the prefix's bytes and clock, or a prefix + live merge
    // would count it twice.
    let ov = idx.open_visit.as_ref().expect("Skyreach is in progress");
    assert_eq!(ov.name, "Skyreach");
    assert_eq!(ov.visit, Some(2));
    assert_eq!(ov.end_ms, None);
    assert_eq!(
        ov.byte_range.1, idx.live_offset,
        "prefix ends where the live tail begins"
    );
    assert_eq!(
        ov.duration_ms, 30_000,
        "closed members only: 0s Skyblade + 30s Ranjit"
    );
}

#[test]
fn a_lazily_loaded_overall_matches_the_full_replay() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    for (meta, ordinal) in idx.overalls.iter().map(|m| (m, m.visit.unwrap())) {
        let lines = load_segment(std::path::Path::new(INSTANCE_FIXTURE), meta).unwrap();
        let lazy = meter_from_lines(lines.iter().map(String::as_str));
        let got = lazy.overall(ordinal).expect("lazy replay finds the visit");
        let want = meter.overall(ordinal).unwrap();
        for view in [View::Damage, View::Healing, View::Deaths, View::Taken] {
            assert_eq!(
                amounts(&got, view),
                amounts(&want, view),
                "{:?} in {}",
                view,
                meta.name
            );
        }
        // R17: the merged mitigation records agree too (raw-keyed, folded
        // on read — a lazy Overall must fold exactly like the full one).
        for r in want.rows(View::Taken) {
            assert_eq!(
                got.mitigation(&r.key),
                want.mitigation(&r.key),
                "R17 mitigation for {} in {}",
                r.label,
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

/// The daemon attaches mid-visit by composing two halves: the lazily loaded
/// `open_visit` prefix, and the live meter (seed lines + everything from
/// `live_offset`, which rebuilds the open member in full). Their merged
/// Overall must equal a full replay — counters and clock. Double counting
/// the scan-time-open member's head is the regression this gates.
#[test]
fn an_attach_mid_visit_composes_to_the_full_replay() {
    let bytes = std::fs::read(INSTANCE_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    // The prefix, loaded the way the daemon's loader does.
    let ov = idx.open_visit.as_ref().expect("Skyreach is in progress");
    let prefix_lines = load_segment(std::path::Path::new(INSTANCE_FIXTURE), ov).unwrap();
    let prefix = meter_from_lines(prefix_lines.iter().map(String::as_str));

    // The live side, built the way the tailer feeds it: the open segment's
    // seed lines, then everything from `live_offset`.
    let seeds = &idx.open.as_ref().expect("trailing pull is open").seeds;
    let mut live_text = String::new();
    for &(s, e) in seeds {
        live_text.push_str(std::str::from_utf8(&bytes[s as usize..e as usize]).unwrap());
    }
    live_text.push_str(std::str::from_utf8(&bytes[idx.live_offset as usize..]).unwrap());
    let live = meter_from_lines(live_text.lines());

    // The daemon's LiveOverall merge: live members + the lazy prefix.
    let mut combined = live.overall(2).expect("the open pull is a live member");
    combined.absorb(&prefix.overall(2).expect("prefix holds the closed members"));

    let want = meter.overall(2).unwrap();
    assert_eq!(
        amounts(&combined, View::Damage),
        amounts(&want, View::Damage),
        "no member counted twice, none dropped"
    );
    assert_eq!(
        combined.duration_ms(0),
        want.duration_ms(i64::MAX),
        "the visit clock survives the split"
    );
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
