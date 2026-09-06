//! R21 over a real log: every `_DOSE` line parses to `AuraDose` (a census —
//! an unparsed one is a format drift), and on every boss pull, for every
//! player, Σ a cell group's hits / sum never exceed the unconditioned Taken
//! row (the derived level 0 is never negative) and a debuff's `hits` is Σ
//! its cells'. Prints the pull's stacking debuffs (max level ≥ 2) so the
//! numbers sit beside a PR.
//!
//! Run: `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release
//! -p wowdps-core --test real_log_stacks -- --ignored --nocapture`

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{SegmentKind, View, meter_from_lines};
use wowdps_core::parser::{Event, parse_line};

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn stack_cells_are_bounded_on_every_real_boss_pull() {
    let path = std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG");
    let mut file = std::fs::File::open(&path).expect("open the log");
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
    for meta in &pulls {
        let text = load_segment_text(Path::new(&path), meta).expect("load the pull");
        for raw in text.lines() {
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
        let meter = meter_from_lines(text.lines());
        let mut keys: HashSet<String> = HashSet::new();
        for seg in meter.segments() {
            for r in seg.rows(View::Taken) {
                keys.insert(r.key.clone());
            }
        }
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
                for ((aura, label), (hits, sum)) in &groups {
                    let row = by_spell
                        .iter()
                        .find(|r| r.label == *label)
                        .unwrap_or_else(|| panic!("{}: {k}: no Taken row {label}", meta.name));
                    assert!(
                        u64::from(*hits) <= row.count && *sum <= row.amount,
                        "{}: {k} aura {aura} {label}: {hits}/{sum} over {}/{}",
                        meta.name,
                        row.count,
                        row.amount
                    );
                    checked += 1;
                }
                for d in seg.stacking_debuffs(k) {
                    let want: u32 = cells
                        .iter()
                        .filter(|c| c.aura_spell_id == d.spell_id)
                        .map(|c| c.hits)
                        .sum();
                    assert_eq!(d.hits, want, "{}: {k} debuff {}", meta.name, d.spell_id);
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
