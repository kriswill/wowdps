//! R13: arena matches, against the committed arena fixture — segment
//! boundaries and naming, the win/loss verdict, scanner parity, lazy-load
//! parity, and checkpoint resumption (the resumed scan must remember the
//! arena's zone name across a cut, which is what `ScanState::last_zone`
//! exists for).

use wowdps_core::index::{load_segment, scan, scan_from};
use wowdps_core::meter::{Meter, SegmentKind, View, meter_from_lines};

const ARENA_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/arena.txt");

fn replay() -> Meter {
    let text = std::fs::read_to_string(ARENA_FIXTURE);
    assert!(text.is_ok(), "{ARENA_FIXTURE}: unreadable fixture");
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
fn matches_become_named_encounter_segments_with_verdicts() {
    let meter = replay();
    let segs = meter.segments();

    let tags: Vec<(SegmentKind, &str, Option<bool>)> = segs
        .iter()
        .map(|s| (s.kind, s.name.as_str(), s.success))
        .collect();
    assert_eq!(
        tags,
        vec![
            // Gate-prep healing stays outside the match.
            (SegmentKind::Trash, "Trash", None),
            // Our side is team 1, END names team 1: a win.
            (
                SegmentKind::Encounter,
                "Ashamane's Fall (Skirmish)",
                Some(true)
            ),
            // Home side 0 (COMBATANT_INFO faction), END names side 1: a loss.
            (
                SegmentKind::Encounter,
                "Dalaran Arena (Skirmish)",
                Some(false)
            ),
            // Post-match pet/DoT tail before the teleport out: noise.
            (SegmentKind::Trash, "Trash", None),
            // Still fighting at EOF.
            (SegmentKind::Encounter, "Empyrean Domain (Skirmish)", None),
        ]
    );

    // R13: the tail records into a segment that exists (ids stay positional)
    // but never counts — even though its player-vs-player damage would have
    // satisfied R11.
    assert!(segs[3].noise);
    assert!(!segs[3].counts());
    assert!(!segs[4].noise, "the live match after the tail is real");

    // Arena zones log difficulty 0, so no R10 visit opens.
    assert!(meter.visits().is_empty());
    assert!(segs.iter().all(|s| s.visit.is_none()));

    // R13: matches are marked arena (headers word success WIN/LOSS); the
    // prep segment is not.
    let arenas: Vec<bool> = segs.iter().map(|s| s.arena).collect();
    assert_eq!(arenas, vec![false, true, true, false, true]);

    // R7: a match clocks START..END, even across a >60s damage lull that
    // would have split a Trash segment (dampening games are slow).
    assert_eq!(segs[1].duration_ms(i64::MAX), 60_000);
    assert_eq!(segs[2].duration_ms(i64::MAX), 120_000);
    assert!(segs[4].end_ms.is_none(), "live match open at EOF");

    // R13 teams: enemy players row up too, flagged by the hostile reaction
    // bit, and the sort groups the friendly team ahead of the enemy team even
    // though Xar out-damaged everyone — a renderer splits at the first
    // `enemy` row.
    let damage: Vec<(String, u64, bool)> = segs[1]
        .rows(View::Damage)
        .into_iter()
        .map(|r| (r.label, r.amount, r.enemy))
        .collect();
    assert_eq!(
        damage,
        vec![
            ("Ana-Realm".to_string(), 400, false),
            ("Borin-Realm".to_string(), 300, false),
            ("Xar-Realm".to_string(), 1000, true),
        ]
    );
}

#[test]
fn the_scanner_mirrors_arena_segmentation() {
    let bytes = std::fs::read(ARENA_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    let scanned: Vec<(SegmentKind, String, Option<bool>, bool)> = idx
        .segments
        .iter()
        .chain(idx.open.iter())
        .map(|m| (m.kind, m.name.clone(), m.success, m.arena))
        .collect();
    let replayed: Vec<(SegmentKind, String, Option<bool>, bool)> = meter
        .segments()
        .iter()
        .map(|s| (s.kind, s.name.clone(), s.success, s.arena))
        .collect();
    assert_eq!(scanned, replayed);
    assert!(
        meter.segments().iter().all(|s| s.encounter.is_none()),
        "arena matches carry no ENCOUNTER_START identity"
    );

    // Encounter identity (id / difficulty / group size) mirrors too.
    let scanned_enc: Vec<_> = idx
        .segments
        .iter()
        .chain(idx.open.iter())
        .map(|m| m.encounter)
        .collect();
    let replayed_enc: Vec<_> = meter.segments().iter().map(|s| s.encounter).collect();
    assert_eq!(scanned_enc, replayed_enc);

    // The matches count (Encounter kind); the heal-only prep does not (R11)
    // and neither does the post-match tail (R13 noise) despite its PvP hits.
    let counts: Vec<bool> = idx
        .segments
        .iter()
        .chain(idx.open.iter())
        .map(|m| m.counts)
        .collect();
    assert_eq!(counts, vec![false, true, true, false, true]);

    // The live match is the open segment; the tail replays it from its
    // ARENA_MATCH_START line, so the title rebuilds from the seeded
    // ZONE_CHANGE lines.
    let open = idx.open.as_ref().expect("match open at EOF");
    assert_eq!(open.name, "Empyrean Domain (Skirmish)");
    assert_eq!(open.byte_range.0, idx.live_offset);
}

#[test]
fn a_lazily_loaded_match_matches_the_full_replay() {
    let bytes = std::fs::read(ARENA_FIXTURE).unwrap();
    let idx = scan(&mut &bytes[..]);
    let meter = replay();

    for (i, meta) in idx.segments.iter().enumerate() {
        let lines = load_segment(std::path::Path::new(ARENA_FIXTURE), meta).unwrap();
        let lazy = meter_from_lines(lines.iter().map(String::as_str));
        let got = lazy.segments().last().expect("slice rebuilds the segment");
        let want = &meter.segments()[i];
        assert_eq!(got.name, want.name, "segment {i}");
        assert_eq!(got.success, want.success, "segment {i}");
        assert_eq!(
            got.duration_ms(i64::MAX),
            want.duration_ms(i64::MAX),
            "segment {i}"
        );
        for view in [View::Damage, View::Healing] {
            assert_eq!(
                amounts(got, view),
                amounts(want, view),
                "{view:?} in {}",
                meta.name
            );
        }
    }
}

#[test]
fn a_resumed_scan_matches_a_full_scan_between_matches() {
    let bytes = std::fs::read(ARENA_FIXTURE).unwrap();
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
        assert_eq!(resumed.open, full.open, "cut at {cut}");
        assert_eq!(resumed.checkpoint, full.checkpoint, "cut at {cut}");
    }
}
