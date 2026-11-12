//! R21 over a real log: every `_DOSE` line parses to `AuraDose` (a census —
//! an unparsed one is a format drift), and on every boss pull, for every
//! player, Σ a cell group's hits / sum never exceed the unconditioned Taken
//! row (the derived level 0 is never negative) and a debuff's `hits` is Σ
//! its cells'. Prints the pull's stacking debuffs (max level ≥ 2) so the
//! numbers sit beside a PR.
//!
//! Its Forever variant holds a Forever log (`WOWDPS_REAL_LOG_FOREVER`) to the
//! same, naming no player, pet or fight by name (`real_log/mod.rs`): a
//! player by their place in the pull's roster, a cell by its aura's id.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_stacks -- --ignored --nocapture`

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
use wowdps_core::meter::{SegmentKind, View};
use wowdps_core::parser::{Event, parse_line};

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn stack_cells_are_bounded_on_every_real_boss_pull() {
    let Some(path) = real_log::retail_log() else {
        return;
    };
    gate(&path, false);
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn stack_cells_are_bounded_on_every_real_forever_boss_pull() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    gate(&path, true);
}

/// The gate over `path`; `anon` prints no name and no guid: a pull by its
/// place in the log, a player by their place in its roster, a cell by its
/// aura's id alone.
fn gate(path: &str, anon: bool) {
    let mut file = std::fs::File::open(path).expect("open the log");
    let idx = scan(&mut file);
    let pulls: Vec<_> = idx
        .segments
        .iter()
        .filter(|m| m.kind == SegmentKind::Encounter)
        .collect();
    assert!(!pulls.is_empty(), "a real log has boss pulls");

    let mut dose_lines = 0u64;
    let mut dose_parsed = 0u64;
    let mut checked = 0usize;
    let mut stacking: BTreeMap<String, u16> = BTreeMap::new();
    for (n, meta) in pulls.iter().enumerate() {
        let text = load_segment_text(Path::new(path), meta).expect("load the pull");
        for raw in text.seeds().chain(text.slice()) {
            let is_dose = raw
                .split("  ")
                .nth(1)
                .and_then(|r| r.split(',').next())
                .is_some_and(|ev| ev.ends_with("_DOSE"));
            if !is_dose {
                continue;
            }
            dose_lines += 1;
            if let Some(l) = parse_line(raw)
                && matches!(l.event, Event::AuraDose { .. })
            {
                dose_parsed += 1;
            }
        }
        let meter = text.meter();
        let mut keys: HashSet<String> = HashSet::new();
        for seg in meter.segments() {
            for r in seg.rows(View::Taken) {
                keys.insert(r.key.clone());
            }
        }
        let mut roster: Vec<&String> = keys.iter().collect();
        roster.sort();
        let pull = if anon {
            format!("pull #{n}")
        } else {
            meta.name.clone()
        };
        let who = |k: &str| {
            if anon {
                real_log::place(&roster, k)
            } else {
                k.to_string()
            }
        };
        let cell = |label: &str| if anon { "" } else { label }.to_string();
        for seg in meter.segments() {
            for k in &keys {
                let cells = seg.stack_cells(k);
                if cells.is_empty() {
                    continue;
                }
                let (by_spell, _) = seg.breakdown(k, View::Taken);
                let mut groups: BTreeMap<(u32, String), (u32, u64)> = BTreeMap::new();
                for c in &cells {
                    let g = groups
                        .entry((c.aura_spell_id, c.damage_label.clone()))
                        .or_default();
                    g.0 += c.hits;
                    g.1 += c.sum;
                }
                // A pet's hits fold onto the owner under the PLAIN label in
                // the ledger, where the Taken by-ability list keeps them
                // apart as "{spell} ({pet})" (R5) — so a cell group is
                // bounded by the rows summed by base KEY ("spell\0pet"),
                // never by label, which can hold " (" of its own.
                let mut rows_by_base: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
                for r in &by_spell {
                    let base = r.key.split('\u{0}').next().unwrap_or(&r.key);
                    let e = rows_by_base.entry(base).or_default();
                    e.0 += r.count;
                    e.1 += r.amount;
                }
                for ((aura, label), (hits, sum)) in &groups {
                    let (count, amount) =
                        rows_by_base
                            .get(label.as_str())
                            .copied()
                            .unwrap_or_else(|| {
                                panic!(
                                    "{pull}: {}: aura {aura}: no Taken row {}",
                                    who(k),
                                    cell(label)
                                )
                            });
                    assert!(
                        u64::from(*hits) <= count && *sum <= amount,
                        "{pull}: {} aura {aura} {}: {hits}/{sum} over {count}/{amount}",
                        who(k),
                        cell(label),
                    );
                    checked += 1;
                }
                for d in seg.stacking_debuffs(k) {
                    let want: u32 = cells
                        .iter()
                        .filter(|c| c.aura_spell_id == d.spell_id)
                        .map(|c| c.hits)
                        .sum();
                    assert_eq!(d.hits, want, "{pull}: {} debuff {}", who(k), d.spell_id);
                    if d.max_level >= 2 {
                        let e = stacking.entry(d.label.clone()).or_default();
                        *e = (*e).max(d.max_level);
                    }
                }
            }
        }
    }
    println!(
        "dose lines {dose_lines}, parsed {dose_parsed}; {checked} cell groups checked; \
         stacking debuffs on players: {stacking:?}"
    );
    assert_eq!(
        dose_lines, dose_parsed,
        "every _DOSE line parses to AuraDose"
    );
}
