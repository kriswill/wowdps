//! R17 over a real log (`docs/plan-role-pivots-step2.md` §8): the
//! taken = dealt identity on every boss pull, a miss-line census (every
//! `*_MISSED` line must parse to `Missed` or, for an unknown kind, `Other`),
//! and the wall time of the Taken-bearing parse so a regression shows up
//! beside the numbers in a PR.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_taken -- --ignored --nocapture`

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, View, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};

const MISSED: [&str; 5] = [
    "SWING_MISSED",
    "SPELL_MISSED",
    "SPELL_PERIODIC_MISSED",
    "RANGE_MISSED",
    "DAMAGE_SHIELD_MISSED",
];

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn taken_equals_dealt_on_every_real_boss_pull() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let pulls: Vec<_> = idx
        .segments
        .iter()
        .filter(|m| m.kind == SegmentKind::Encounter)
        .collect();
    assert!(!pulls.is_empty(), "a real log has boss pulls");

    let mut missed_lines = 0u64;
    let mut missed_parsed = 0u64;
    let mut missed_other = 0u64;
    let mut unknown_kinds: HashSet<String> = HashSet::new();
    let mut parse_ms = 0u128;
    let mut taken_total = 0u64;
    let mut checked = 0usize;

    for meta in &pulls {
        let text = load_segment_text(Path::new(&path), meta).expect("load the pull");
        let t = Instant::now();
        let lines: Vec<_> = text.lines().filter_map(parse_line).collect();
        let meter = meter_from_lines(text.lines());
        parse_ms += t.elapsed().as_millis();

        // Census: every miss line is Missed, or Other for a kind the model
        // does not know (report which, never fail on it).
        for raw in text.lines() {
            let Some(ev) = raw.split("  ").nth(1).and_then(|r| r.split(',').next()) else {
                continue;
            };
            if !MISSED.contains(&ev) {
                continue;
            }
            missed_lines += 1;
            match parse_line(raw).map(|l| l.event) {
                Some(Event::Missed { .. }) => missed_parsed += 1,
                _ => {
                    missed_other += 1;
                    let kind = raw
                        .split(',')
                        .find(|f| f.chars().all(|c| c.is_ascii_uppercase()) && f.len() > 3)
                        .unwrap_or("?")
                        .to_string();
                    unknown_kinds.insert(kind);
                }
            }
        }

        // The identity, on the public surface, the way tests/taken.rs pins it
        // over the fixtures.
        let mut friendly: HashSet<String> = HashSet::new();
        let mut guids: HashSet<String> = HashSet::new();
        for l in &lines {
            if let Some(h) = &l.owner_hint {
                guids.insert(h.owner_guid.clone());
            }
            match &l.event {
                Event::Damage { src, dst, .. } => {
                    guids.insert(src.guid.clone());
                    if dst.guid.starts_with("Player-") || dst.guid.starts_with("Pet-") {
                        friendly.insert(dst.name.clone());
                    }
                }
                Event::Summon { owner, .. } => {
                    guids.insert(owner.guid.clone());
                }
                _ => {}
            }
        }
        for seg in meter.segments() {
            let dealt: u64 = guids
                .iter()
                .flat_map(|g| seg.breakdown(g, View::Damage).1)
                .filter(|r| friendly.contains(&r.label))
                .map(|r| r.amount)
                .sum();
            let rows = seg.rows(View::Taken);
            let taken: u64 = rows.iter().map(|r| r.amount).sum();
            let ticked: u64 = rows
                .iter()
                .filter_map(|r| seg.mitigation(&r.key))
                .map(|m| m.stagger_ticked)
                .sum();
            assert_eq!(
                dealt,
                taken + ticked,
                "{}: dealt to friendlies vs taken (+ticked {ticked})",
                seg.name
            );
            taken_total += taken;
            checked += 1;
        }
    }

    println!(
        "{} pulls, {checked} segments checked, Σ taken {taken_total}; \
         {missed_lines} miss lines: {missed_parsed} parsed, {missed_other} other \
         (unknown kinds: {unknown_kinds:?}); parse+meter {parse_ms} ms",
        pulls.len()
    );
    assert_eq!(
        missed_other, 0,
        "every observed miss kind should be modeled; unknown: {unknown_kinds:?}"
    );
}
