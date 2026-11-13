//! R31 over a real log: the logger, the character whose unit flags say
//! MINE as a player (`Unit::is_logger`), read off every line, and each
//! fight's logger (`Segment::logger`).
//!
//! Held, in both editions:
//! - no line flags two players as the logger: the bit names one character
//!   at a time;
//! - every closed fight's logger, read by the full replay, is a lazy load's
//!   of the fight alone (lazy = full);
//! - on Retail, every boss pull (an arena aside) that names a logger carries
//!   the logger's COMBATANT_INFO loadout: the flag names a player the owner
//!   inference reads too (1 377 of 1 377 over 83 logs, 2026-11-12).
//!
//! Printed: how many fights of each kind name a logger and how many of
//! those the logger has a row in; how many pulls with a friendly row name
//! none (a raid's pull the writer took no part in: no line of theirs); how
//! many characters the log flags, and how often the logger changes from
//! one fight to the next (a log written by two characters, one after the
//! other); and how many boss pulls' loggers a COMBATANT_INFO named in that
//! pull, the line the owner inference intersects (none on Forever, whose
//! log writes no COMBATANT_INFO).
//!
//! A real log carries real names: nothing here prints a name or a guid, a
//! failure included. A fight is named by its kind and place in the log.
//!
//! Run: `WOWDPS_REAL_LOG=<Retail log> WOWDPS_REAL_LOG_FOREVER=<Forever
//! log> cargo test --release -p wowdps-core --test real_log_logger --
//! --ignored --nocapture` (which log each reads: `tests/real_log/mod.rs`).
//! Both names hold `real`, and the Forever variant's `forever`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod real_log;

use std::collections::BTreeSet;
use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{Segment, SegmentKind, View, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};

/// Whether `guid` has a friendly row on any of the card's four views.
fn has_row(seg: &Segment, guid: &str) -> bool {
    [View::Damage, View::Healing, View::Deaths, View::Taken]
        .into_iter()
        .any(|v| seg.rows(v).iter().any(|r| !r.enemy && r.key == guid))
}

fn friendly_rows(seg: &Segment) -> bool {
    [View::Damage, View::Healing, View::Deaths, View::Taken]
        .into_iter()
        .any(|v| seg.rows(v).iter().any(|r| !r.enemy))
}

fn check(path: &str, edition: &str) {
    let text = std::fs::read_to_string(path).expect("read the log");

    // Line by line: the blocks that flag the logger, and never two players.
    let (mut flagged, mut two) = (0u64, 0u64);
    let mut characters: BTreeSet<String> = BTreeSet::new();
    for line in text.lines() {
        let Some(parsed) = parse_line(line) else {
            continue;
        };
        if matches!(parsed.event, Event::Other) {
            continue;
        }
        let here: BTreeSet<&str> = parsed
            .event
            .units()
            .into_iter()
            .flatten()
            .filter(|u| u.is_logger())
            .map(|u| u.guid.as_str())
            .collect();
        if !here.is_empty() {
            flagged += 1;
        }
        if here.len() > 1 {
            two += 1;
        }
        characters.extend(here.into_iter().map(str::to_string));
    }
    println!(
        "{edition}: {flagged} lines flag the logger, {} characters flagged",
        characters.len()
    );
    assert_eq!(two, 0, "{two} lines flag two players as the logger");

    let meter = meter_from_lines(text.lines());
    let mut file = std::fs::File::open(path).expect("open the log");
    let idx = scan(&mut file);
    let closed: Vec<&Segment> = meter
        .segments()
        .iter()
        .filter(|s| s.end_ms.is_some())
        .collect();
    let metas: Vec<_> = idx.segments.iter().filter(|m| m.end_ms.is_some()).collect();
    assert_eq!(
        closed.len(),
        metas.len(),
        "scan and replay see the same fights"
    );

    let mut changes = 0;
    let mut last: Option<&str> = None;
    // (kind, fights, with a logger, the logger with a row)
    let mut tally: Vec<(&str, usize, usize, usize)> = vec![("pull", 0, 0, 0), ("trash", 0, 0, 0)];
    let (mut ci_pulls, mut ci_named, mut unwritten) = (0, 0, 0);
    for (n, (seg, meta)) in closed.iter().zip(&metas).enumerate() {
        let label = real_log::fight(n, seg);
        assert_eq!(
            (seg.kind, seg.start_ms),
            (meta.kind, meta.start_ms),
            "{label}: scan and replay disagree on the fight"
        );
        let lazy = load_segment_text(Path::new(path), meta)
            .expect("load the fight")
            .meter();
        let part = lazy
            .segments()
            .iter()
            .find(|s| s.kind == seg.kind && s.start_ms == seg.start_ms)
            .unwrap_or_else(|| panic!("{label}: the lazy load holds no fight at its start"));
        assert!(
            part.logger() == seg.logger(),
            "{label}: lazy and full name different loggers (full {}, lazy {})",
            seg.logger().is_some(),
            part.logger().is_some()
        );

        let logger = seg.logger();
        if let Some(g) = logger {
            if last.is_some_and(|l| l != g) {
                changes += 1;
            }
            last = Some(g);
        }
        let row = logger.is_some_and(|g| has_row(seg, g));
        let slot = match seg.kind {
            SegmentKind::Encounter => 0,
            _ => 1,
        };
        let t = &mut tally[slot];
        t.1 += 1;
        t.2 += usize::from(logger.is_some());
        t.3 += usize::from(row);
        if seg.kind == SegmentKind::Encounter {
            unwritten += usize::from(logger.is_none() && friendly_rows(seg));
            if let Some(g) = logger
                && !seg.arena
            {
                ci_pulls += 1;
                ci_named += usize::from(seg.loadout(g).is_some());
            }
        }
    }
    for (kind, fights, with, row) in &tally {
        println!(
            "{edition}: {fights} {kind} fights, {with} name a logger, {row} with the logger's row"
        );
    }
    println!("{edition}: {unwritten} pulls with a friendly row name no logger");
    println!("{edition}: the logger changes {changes} times from one fight to the next");
    println!(
        "{edition}: {ci_named} of {ci_pulls} pulls with a logger carry its COMBATANT_INFO loadout"
    );
    if edition == "retail" {
        assert_eq!(
            ci_named, ci_pulls,
            "a Retail pull's logger with no COMBATANT_INFO loadout in it"
        );
    }
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real Retail combat log"]
fn the_logger_holds_on_a_real_log() {
    let Some(path) = real_log::retail_log() else {
        return;
    };
    check(&path, "retail");
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn the_logger_holds_on_a_real_forever_log() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    check(&path, "forever");
}
