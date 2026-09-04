//! R16: boss health. The advanced block reports the health of the unit it
//! describes; inside an open raid-boss Encounter the lowest fraction seen
//! for a hostile NPC is how far the pull got — 0 on a kill. The fixture
//! carries the numbers: The Ashen Warden dies (a `0/12000000` report on the
//! killing blow's LANDED twin), Verkath the Hollow is left at 8863800/9000000.

#![allow(clippy::unwrap_used)]

use wowdps_core::meter::{Meter, SegmentKind};
use wowdps_core::parser::parse_line;

fn replay(path: &str) -> Meter {
    let text = std::fs::read_to_string(path).unwrap();
    let mut meter = Meter::new();
    for line in text.lines() {
        if let Some(parsed) = parse_line(line) {
            meter.feed(parsed);
        }
    }
    meter
}

#[test]
fn the_fixture_bosses_report_how_low_they_got() {
    let meter = replay(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample.txt"));
    let pcts: Vec<(String, Option<u16>)> = meter
        .segments()
        .iter()
        .map(|s| (s.name.clone(), s.best_pct()))
        .collect();
    assert_eq!(
        pcts,
        vec![
            ("Gloomstalker".to_string(), None),
            ("The Ashen Warden".to_string(), Some(0)),
            ("Hollow Drudge".to_string(), None),
            ("Verkath the Hollow".to_string(), Some(98)),
        ],
        "trash never reports; the kill reached 0; the wipe stopped at 98.48% → 98"
    );
}

#[test]
fn arena_matches_and_keyed_overalls_never_report_boss_health() {
    let arena = replay(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/arena.txt"));
    assert!(arena.segments().iter().all(|s| s.best_pct().is_none()));
    let instance = replay(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/instance.txt"
    ));
    for (ord, _) in instance.visits().iter().enumerate() {
        if let Some(overall) = instance.overall(ord as u32) {
            assert_eq!(overall.kind, SegmentKind::Overall);
            assert_eq!(overall.best_pct(), None, "an Overall is not a boss");
        }
    }
}
