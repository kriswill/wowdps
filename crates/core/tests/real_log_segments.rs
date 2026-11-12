//! Forever over a real log: segmentation and totals, each fight read by
//! that edition's rules. The scan and the full replay cut the log into the
//! same fights (kind, start, end, outcome, encounter); every closed fight
//! the scan lists reads the same loaded alone as in the full replay, row
//! for row in Damage, Healing, Taken and Deaths, and on both clocks; every
//! visit's Overall does too; every fight is Forever's, and no player has a
//! spec, which Forever's log never names. The census says how many players
//! Forever's class table placed.
//!
//! A real log carries real names: nothing here prints a name or a guid, a
//! failure included (`real_log/mod.rs`). A fight is named by its kind, its
//! place and its encounter id; a row by its place in the view.
//!
//! Run: `WOWDPS_REAL_LOG_FOREVER=/path/to/WoWCombatLog-*.txt cargo test
//! --release -p wowdps-core --test real_log_segments -- --ignored
//! --nocapture`

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod real_log;

use std::path::Path;
use std::time::Instant;

use wowdps_core::index::{SegmentMeta, load_segment_text, scan};
use wowdps_core::meter::{Segment, SegmentKind, View, meter_from_lines};
use wowdps_model::Edition;

/// The views whose rows a lazy load must reproduce.
const VIEWS: [View; 4] = [View::Damage, View::Healing, View::Taken, View::Deaths];

/// A view's rows by key, as compared: (key, amount, count). Never printed.
fn rows(seg: &Segment, view: View) -> Vec<(String, u64, u64)> {
    let mut rows: Vec<_> = seg
        .rows(view)
        .into_iter()
        .map(|r| (r.key, r.amount, r.count))
        .collect();
    rows.sort();
    rows
}

/// Where two row lists first differ, by place alone: what a failure says.
fn first_difference(a: &[(String, u64, u64)], b: &[(String, u64, u64)]) -> String {
    if a.len() != b.len() {
        return format!("{} rows full, {} lazy", a.len(), b.len());
    }
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => {
            let (x, y) = (&a[i], &b[i]);
            let same = if x.0 == y.0 { "same" } else { "another" };
            format!(
                "row #{i}: full {} over {}, lazy {} over {} ({same} player)",
                x.1, x.2, y.1, y.2
            )
        }
        None => "no difference".to_string(),
    }
}

/// `lazy` reads as `full` does: every view's rows and both clocks.
fn same_fight(label: &str, full: &Segment, lazy: &Segment) {
    for view in VIEWS {
        let (a, b) = (rows(full, view), rows(lazy, view));
        assert!(
            a == b,
            "{label}: {view:?} differs, {}",
            first_difference(&a, &b)
        );
    }
    let end = full.last_combat_ms();
    assert_eq!(
        full.duration_ms(end),
        lazy.duration_ms(end),
        "{label}: the fight's clock"
    );
    assert_eq!(
        full.combat_ms(end),
        lazy.combat_ms(end),
        "{label}: the combat clock"
    );
    assert_eq!(full.success, lazy.success, "{label}: the outcome");
}

/// The scan's fight and the replay's agree on what the fight is.
fn same_cut(label: &str, meta: &SegmentMeta, seg: &Segment) {
    assert_eq!(meta.end_ms, seg.end_ms, "{label}: the end");
    assert_eq!(meta.success, seg.success, "{label}: the outcome");
    assert_eq!(meta.encounter, seg.encounter, "{label}: the encounter");
    assert_eq!(meta.visit, seg.visit, "{label}: the visit");
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn forever_segments_and_totals_hold_on_a_real_log() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    let t = Instant::now();
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let scan_ms = t.elapsed().as_millis();
    let t = Instant::now();
    let text = std::fs::read_to_string(&path).expect("read the log");
    let meter = meter_from_lines(text.lines());
    let replay_ms = t.elapsed().as_millis();
    let full = meter.segments();
    assert!(!idx.segments.is_empty(), "a real log has closed fights");

    // Every fight is Forever's, read by its tables: a class from its class
    // masks where a spell names one, never a spec.
    let (mut players, mut classed) = (0usize, 0usize);
    for (n, seg) in full.iter().enumerate() {
        let label = real_log::fight(n, seg);
        assert_eq!(
            Edition::from_project_id(seg.project_id),
            Edition::Forever,
            "{label}: the edition"
        );
        for view in [View::Damage, View::Healing] {
            for (i, r) in seg.rows(view).into_iter().enumerate() {
                assert!(r.spec.is_none(), "{label}: {view:?} row #{i} has a spec");
            }
        }
        if seg.kind == SegmentKind::Encounter {
            for r in seg.rows(View::Damage) {
                if r.key.starts_with("Player-") {
                    players += 1;
                    classed += usize::from(r.class.is_some());
                }
            }
        }
    }

    // The scan cuts what the replay cuts: each fight it lists is the
    // replay's fight at that start, the same in every way it records; every
    // closed boss fight of the replay is listed.
    let mut listed = 0usize;
    for (n, meta) in idx.segments.iter().enumerate() {
        let label = format!("listed fight #{n} ({:?})", meta.kind);
        let Some(seg) = full
            .iter()
            .find(|s| s.kind == meta.kind && s.start_ms == meta.start_ms)
        else {
            panic!("{label}: the full replay holds no fight at its start");
        };
        same_cut(&label, meta, seg);
        listed += 1;
    }
    let bosses: Vec<&Segment> = full
        .iter()
        .filter(|s| s.kind == SegmentKind::Encounter && s.end_ms.is_some())
        .collect();
    for (n, seg) in bosses.iter().enumerate() {
        assert!(
            idx.segments
                .iter()
                .any(|m| m.kind == SegmentKind::Encounter && m.start_ms == seg.start_ms),
            "{}: the scan does not list it",
            real_log::fight(n, seg)
        );
    }
    let unlisted = full
        .iter()
        .filter(|s| s.kind == SegmentKind::Trash && s.end_ms.is_some())
        .filter(|s| {
            !idx.segments
                .iter()
                .any(|m| m.kind == SegmentKind::Trash && m.start_ms == s.start_ms)
        })
        .count();

    // Lazy = full on every closed fight the scan lists, and on every
    // visit's Overall.
    let t = Instant::now();
    for (n, meta) in idx.segments.iter().enumerate() {
        let label = format!("listed fight #{n} ({:?})", meta.kind);
        let lazy = load_segment_text(Path::new(&path), meta)
            .expect("load the fight")
            .meter();
        let Some(mine) = lazy
            .segments()
            .iter()
            .find(|s| s.kind == meta.kind && s.start_ms == meta.start_ms)
        else {
            panic!("{label}: the lazy load holds no fight at its start");
        };
        let Some(seg) = full
            .iter()
            .find(|s| s.kind == meta.kind && s.start_ms == meta.start_ms)
        else {
            panic!("{label}: the full replay holds no fight at its start");
        };
        same_fight(&label, seg, mine);
    }
    for (n, meta) in idx.overalls.iter().enumerate() {
        let label = format!("overall #{n}");
        let visit = meta.visit.expect("an Overall names its visit");
        let lazy = load_segment_text(Path::new(&path), meta)
            .expect("load the visit")
            .meter();
        let got = lazy.overall(visit).expect("the lazy load holds the visit");
        let want = meter.overall(visit).expect("the replay holds the visit");
        same_fight(&label, &want, &got);
    }
    let lazy_ms = t.elapsed().as_millis();

    let in_visit = full.iter().filter(|s| s.visit.is_some()).count();
    let damage: u64 = bosses
        .iter()
        .flat_map(|s| s.rows(View::Damage))
        .map(|r| r.amount)
        .sum();
    println!(
        "{} fights ({} boss fights, {in_visit} inside an instance visit, {} overalls); {listed} \
         listed by the scan, {unlisted} closed \
         trash fights it does not list; {classed} of {players} players' boss rows classed by \
         Forever's table; Σ boss damage {damage}; scan {scan_ms} ms, full replay {replay_ms} ms, \
         lazy loads {lazy_ms} ms",
        full.len(),
        bosses.len(),
        idx.overalls.len(),
    );
}
