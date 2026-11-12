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
//! Its Forever variant holds a Forever log (`WOWDPS_REAL_LOG_FOREVER`) to the
//! same, naming no player and no fight by name (`real_log/mod.rs`): a
//! player by their place in the segment's roster. That edition has no
//! empowered spells, so its empower counts hold at zero.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_power -- --ignored --nocapture`

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod real_log;

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
    let Some(path) = real_log::retail_log() else {
        return;
    };
    gate(&path, false);
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn heal_absorbs_and_power_hold_on_every_real_forever_segment() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    gate(&path, true);
}

/// The gate over `path`; `anon` prints no name and no guid: a segment by
/// its place in the log, a player by their place in its roster, a row by
/// its place in its view.
fn gate(path: &str, anon: bool) {
    let mut file = std::fs::File::open(path).expect("open the log");
    let idx = scan(&mut file);
    let metas: Vec<_> = idx.segments.iter().chain(idx.open.as_ref()).collect();
    assert!(!metas.is_empty(), "a real log has segments");
    let mut r = Report::default();

    for (n, meta) in metas.iter().enumerate() {
        let text = load_segment_text(Path::new(path), meta).expect("load the segment");
        let meter = text.meter();
        let here = if anon {
            format!("segment #{n}")
        } else {
            meta.name.clone()
        };
        assert_eq!(meter.segments().len(), 1, "{here}: one segment");
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
        for (i, row) in seg.rows(View::Healing).into_iter().enumerate() {
            let shown = if anon {
                format!("healing row #{i}")
            } else {
                format!("{row:?}")
            };
            assert!(
                row.heal_absorbed <= row.amount,
                "{here}: {shown}: {} eaten of {}",
                row.heal_absorbed,
                row.amount
            );
            let (spells, _) = seg.breakdown(&row.key, View::Healing);
            let parts: u64 = spells.iter().map(|s| s.heal_absorbed).sum();
            assert_eq!(parts, row.heal_absorbed, "{here}: {shown}");
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
        let mut roster: Vec<&String> = players.iter().collect();
        roster.sort();
        let who = |p: &str| {
            if anon {
                real_log::place(&roster, p)
            } else {
                p.to_string()
            }
        };
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
                        let spell = if anon { String::new() } else { m.key.clone() };
                        assert!(*row <= all, "{here}: {spell} on {}", who(p));
                    }
                    assert!(m.empower.cancelled <= e.cancelled);
                }
            }
            for s in seg.power(p) {
                assert!(p.starts_with("Player-"), "{here}: {} has a pool", who(p));
                assert!(
                    reported.contains(&(p.clone(), s.power_type)),
                    "{here}: {} type {} never reported",
                    who(p),
                    s.power_type
                );
                assert!(
                    s.per_sec.len() <= seconds.max(1) + 120,
                    "{here}: {} seconds for {}",
                    s.per_sec.len(),
                    who(p)
                );
                assert!(s.max > 0 && s.reported() > 0);
                r.series += 1;
                *r.by_type.entry(s.power_type).or_default() += 1;
            }
        }
        assert!(
            released + cancelled <= empower_lines,
            "{here}: {released} + {cancelled} counted of {empower_lines} lines"
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
