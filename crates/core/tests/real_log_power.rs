//! v44 over a real log — R2's heal-absorbed split, R26's empowered stages and
//! R28's power series — on EVERY segment, each loaded lazily as the daemon
//! loads it: the eaten part of every Healing row is at most its healing and
//! its abilities' parts sum to it (the census beside it counts the heal lines
//! whose `absorbed` exceeds what they healed, which the cap keeps out); the
//! empowered releases and cancels on the rows never exceed the player's, and
//! the player's never exceed the `SPELL_EMPOWER_END` / `_INTERRUPT` lines the
//! slice holds; every power series is a player's own, no longer than the
//! segment's seconds, and of a type the advanced block reported for them.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_power -- --ignored --nocapture`

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::View;
use wowdps_core::parser::{Event, parse_line};

#[derive(Default)]
struct Report {
    segments: usize,
    healing: u64,
    heal_absorbed: u64,
    capped_lines: u64,
    empower_released: u64,
    empower_cancelled: u64,
    empower_lines: u64,
    series: usize,
    by_type: BTreeMap<u32, usize>,
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn heal_absorbs_empowers_and_power_hold_on_every_real_segment() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
    let idx = scan(&mut file);
    let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
    assert!(!metas.is_empty(), "a real log has segments");
    let mut r = Report::default();

    for meta in &metas {
        let text = load_segment_text(Path::new(&path), meta).expect("load the segment");
        let meter = text.meter();
        assert_eq!(meter.segments().len(), 1, "{}: one segment", meta.name);
        let seg = &meter.segments()[0];
        r.segments += 1;

        // What the slice itself says: the empower lines, the heal lines that
        // logged more eaten than they healed, the power types per player.
        let mut empower_lines = 0u64;
        let mut reported: HashSet<(String, u32)> = HashSet::new();
        for line in text.slice().filter_map(parse_line) {
            match &line.event {
                Event::Empower {
                    stage, interrupted, ..
                } if *interrupted || stage.is_some() => empower_lines += 1,
                Event::Heal {
                    amount,
                    overheal,
                    absorbed,
                    ..
                } if *absorbed > amount.saturating_sub(*overheal) => r.capped_lines += 1,
                _ => {}
            }
            if let Some(h) = &line.hp_hint
                && let Some(p) = h.power
            {
                reported.insert((h.unit_guid.clone(), p.kind));
            }
        }
        r.empower_lines += empower_lines;

        // R2: the eaten part is inside the healing, and the drill agrees.
        for row in seg.rows(View::Healing) {
            assert!(row.heal_absorbed <= row.amount, "{}: {row:?}", meta.name);
            let (spells, _) = seg.breakdown(&row.key, View::Healing);
            let parts: u64 = spells.iter().map(|s| s.heal_absorbed).sum();
            assert_eq!(parts, row.heal_absorbed, "{}: {}", meta.name, row.label);
            r.healing += row.amount;
            r.heal_absorbed += row.heal_absorbed;
        }

        // R26 and R28 per player of any view.
        let mut players: HashSet<String> = HashSet::new();
        for view in View::ALL {
            if view != View::EnemyTaken {
                players.extend(seg.rows(view).into_iter().map(|row| row.key));
            }
        }
        let seconds = (seg.duration_ms(seg.last_combat_ms()) / 1000 + 2) as usize;
        let mut released = 0;
        let mut cancelled = 0;
        for p in &players {
            let e = seg.empower(p);
            released += e.released();
            cancelled += e.cancelled;
            for view in [View::Damage, View::Healing] {
                for m in seg.spell_tree(p, view).rows {
                    for (row, all) in m.empower.stages.iter().zip(e.stages) {
                        assert!(*row <= all, "{}: {} on {}", meta.name, m.key, p);
                    }
                    assert!(m.empower.cancelled <= e.cancelled);
                }
            }
            for s in seg.power(p) {
                assert!(p.starts_with("Player-"), "{}: {p} has a pool", meta.name);
                assert!(
                    reported.contains(&(p.clone(), s.power_type)),
                    "{}: {p} type {} never reported",
                    meta.name,
                    s.power_type
                );
                assert!(
                    s.per_sec.len() <= seconds.max(1) + 120,
                    "{}: {} seconds for {p}",
                    meta.name,
                    s.per_sec.len()
                );
                assert!(s.max > 0 && s.reported() > 0);
                r.series += 1;
                *r.by_type.entry(s.power_type).or_default() += 1;
            }
        }
        assert!(
            released + cancelled <= empower_lines,
            "{}: {released} + {cancelled} counted of {empower_lines} lines",
            meta.name
        );
        r.empower_released += released;
        r.empower_cancelled += cancelled;
    }

    println!(
        "{} segments; Σ healing {} of which heal-absorbed {} ({:.2} %), {} heal lines capped; \
         empowers: {} released + {} cancelled of {} lines; {} power series by type {:?}",
        r.segments,
        r.healing,
        r.heal_absorbed,
        if r.healing > 0 {
            r.heal_absorbed as f64 * 100.0 / r.healing as f64
        } else {
            0.0
        },
        r.capped_lines,
        r.empower_released,
        r.empower_cancelled,
        r.empower_lines,
        r.series,
        r.by_type,
    );
}
