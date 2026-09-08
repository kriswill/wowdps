//! R23 over a real log (the real-log gate): every death span a real pull
//! produces is sane — non-negative, inside its segment, one per death the
//! recap kept — and the census says how they ENDED, which is the whole
//! question the ruling turns on. A dead player's DoTs keep ticking (a real
//! log's first tick lands ~200 ms after UNIT_DIED), so a run where most
//! spans read under a second means the liveness signals are wrong again.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_deaths -- --ignored --nocapture`

use std::collections::BTreeMap;
use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};
use wowdps_model::MarkKind;

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn death_spans_are_sane_on_every_real_segment() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
    assert!(!metas.is_empty(), "a real log has segments");

    let mut deaths = 0usize;
    let mut rezzed = 0usize;
    let mut instant = 0usize;
    let mut to_the_end = 0usize;
    let mut longest = 0i64;
    let mut by_rez: BTreeMap<String, u32> = BTreeMap::new();

    for meta in &metas {
        let text = load_segment_text(Path::new(&path), meta).expect("load the segment");
        let meter = meter_from_lines(text.lines());
        let Some(seg) = meter.segments().last() else {
            continue;
        };
        let duration = seg.duration_ms(seg.last_combat_ms());
        // Everyone the log names as dying in this slice — the universe the
        // spans must match, taken from the lines rather than the meter so
        // the test can disagree with it.
        let died: Vec<String> = text
            .lines()
            .filter_map(parse_line)
            .filter_map(|l| match l.event {
                Event::Death { unit } if unit.is_player() => Some(unit.guid),
                _ => None,
            })
            .collect();
        for guid in &died {
            let marks: Vec<_> = seg
                .timeline(guid)
                .marks
                .into_iter()
                .filter(|m| m.kind == MarkKind::Death)
                .collect();
            for m in &marks {
                deaths += 1;
                assert!(m.dur_ms >= 0, "a death lasting negative time: {m:?}");
                assert!(
                    m.at_ms >= 0 && m.at_ms <= duration.max(0),
                    "a death outside its segment: {m:?} of {duration} ms"
                );
                longest = longest.max(m.dur_ms);
                if m.dur_ms < 1_000 {
                    instant += 1;
                }
                if m.at_ms + m.dur_ms >= duration {
                    to_the_end += 1;
                }
                if m.label != "Death" {
                    rezzed += 1;
                    *by_rez.entry(m.label.clone()).or_default() += 1;
                    assert!(
                        m.dur_ms > 0,
                        "a rez that ended a death before it started: {m:?}"
                    );
                }
            }
        }
        if seg.kind == SegmentKind::Encounter && !died.is_empty() {
            assert!(
                deaths > 0,
                "players died in {} and no span says so",
                seg.name
            );
        }
    }

    println!("death spans: {deaths} over {} segments", metas.len());
    println!("  ended by a rez: {rezzed}");
    for (spell, n) in &by_rez {
        println!("    {spell}: {n}");
    }
    println!("  ran to the fight's end: {to_the_end}");
    println!("  under a second: {instant}");
    println!("  longest: {:.1}s", longest as f64 / 1000.0);
    // The regression this file exists for: DoT ticks off a corpse used to
    // close every span within ~200 ms. A real log's deaths are not all
    // instant, so a run where they are means the liveness rule broke.
    if deaths >= 5 {
        assert!(
            instant * 2 < deaths,
            "{instant} of {deaths} deaths lasted under a second — \
             something is closing spans that should not"
        );
    }
}
