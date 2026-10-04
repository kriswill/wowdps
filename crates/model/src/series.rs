//! v39: a fight's abilities (and targets) second by second, and the ONE
//! definition of what a zoom window makes of them. The live meter folds
//! its sparse per-spell series into [`SeriesRow`]s and windows them here
//! (`Segment::spells_in`, `damage_targets_in`); the history store keeps the
//! same rows in its series tier and windows them here too — so a stored
//! pull's zoom answers exactly what the live one did.

use crate::{AbilitySeries, Class, Row, Spec, SpellTree};

/// The grid every series is bucketed on (R12's): one second.
pub const BUCKET_MS: i64 = 1_000;

/// The longest grid a curve keeps: six hours of seconds. A second past it
/// (a clock that jumped) is in no curve, live or stored.
pub const MAX_BUCKETS: usize = 21_600;

/// One second of one row: what landed in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeriesCell {
    /// The second, counted from the segment's start.
    pub bucket: u32,
    pub amount: u64,
    /// Overkill on damage, overheal on healing; 0 on a target row.
    pub extra: u64,
    /// Contributing events, and the ones that crit.
    pub count: u64,
    pub crits: u64,
}

/// One by-ability (or by-target) row's seconds, keyed as the whole-fight
/// row is ("spell", "spell\0pet", or a target's name), oldest first, one
/// cell per second at most.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeriesRow {
    pub key: String,
    pub spell_id: u32,
    pub school: u32,
    pub cells: Vec<SeriesCell>,
}

impl SeriesRow {
    /// The label the whole-fight row wears: a pet's ability is
    /// "{spell} ({pet})", everything else its key.
    pub fn label(&self) -> String {
        match self.key.split_once('\u{0}') {
            Some((spell, pet)) => format!("{spell} ({pet})"),
            None => self.key.clone(),
        }
    }
}

/// v42: one Damage ability's targets second by second — every enemy name
/// it landed on, keyed as the opened ability's target rows are, each
/// row wearing the ability's school. What an opened ability's target list
/// windows ([`window_rows`]) and its graph stacks ([`rank_curves`]), live
/// and stored alike.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpellTargets {
    /// The by-ability row's key ("spell" or "spell\0pet").
    pub key: String,
    pub targets: Vec<SeriesRow>,
}

/// v42: one target's whole-fight tally of one ability.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetTally {
    pub target: String,
    pub amount: u64,
    /// Overkill on damage, overheal on healing.
    pub extra: u64,
    pub count: u64,
    pub crits: u64,
}

/// v42: one ability's whole-fight targets, as tallies — what an opened
/// ability's target list says over the whole fight ([`target_rows`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpellTallies {
    /// The by-ability row's key ("spell" or "spell\0pet").
    pub key: String,
    pub school: u32,
    pub targets: Vec<TargetTally>,
}

/// `cells`' amounts added onto `into` on the grid — second i at index i,
/// `into` grown to the last second a cell names — what one by-ability row
/// draws.
pub fn add_cells(into: &mut Vec<u64>, cells: &[SeriesCell]) {
    for c in cells {
        let i = c.bucket as usize;
        if i >= MAX_BUCKETS {
            continue;
        }
        if into.len() <= i {
            into.resize(i + 1, 0);
        }
        if let Some(slot) = into.get_mut(i) {
            *slot += c.amount;
        }
    }
}

/// The curve of the row keyed `key` among `rows`; empty when none is.
pub fn curve_of(rows: &[SeriesRow], key: &str) -> Vec<u64> {
    let mut out = Vec::new();
    if let Some(r) = rows.iter().find(|r| r.key == key) {
        add_cells(&mut out, &r.cells);
    }
    out
}

/// R26 (step 2): the curves a drill's graph stacks — the `top` largest
/// entries of the ability tree over `rows` (a group's members summed, a
/// row alone), largest first (ties by key), each the sum of its rows'
/// curves, which `curve` gives by row key. An entry whose curve is all
/// zero is left out. The ONE ranking, live and stored.
pub fn stack(
    rows: &[Row],
    tree: &SpellTree,
    top: usize,
    curve: impl Fn(&str) -> Vec<u64>,
) -> Vec<AbilitySeries> {
    let mut entries: Vec<(u64, crate::TreeEntry)> = tree
        .entries(rows)
        .into_iter()
        .map(|e| {
            let sum = e
                .members
                .iter()
                .filter_map(|&i| rows.get(i))
                .map(|r| r.amount)
                .sum();
            (sum, e)
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key.cmp(&b.1.key)));
    entries
        .into_iter()
        .take(top)
        .filter_map(|(_, e)| {
            let mut buckets: Vec<u64> = Vec::new();
            for r in e.members.iter().filter_map(|&i| rows.get(i)) {
                let c = curve(&r.key);
                if buckets.len() < c.len() {
                    buckets.resize(c.len(), 0);
                }
                for (s, v) in buckets.iter_mut().zip(&c) {
                    *s += v;
                }
            }
            buckets.iter().any(|b| *b > 0).then_some(AbilitySeries {
                key: e.key,
                buckets,
            })
        })
        .collect()
}

/// R26 (step 2): the `top` largest of `curves` by their sum, largest first
/// (ties by key), an all-zero one left out — an opened ability's stack by
/// target.
pub fn rank_curves(
    curves: impl IntoIterator<Item = (String, Vec<u64>)>,
    top: usize,
) -> Vec<AbilitySeries> {
    let mut ranked: Vec<(u64, AbilitySeries)> = curves
        .into_iter()
        .map(|(key, buckets)| (buckets.iter().sum(), AbilitySeries { key, buckets }))
        .filter(|(sum, _)| *sum > 0)
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key.cmp(&b.1.key)));
    ranked.into_iter().take(top).map(|(_, s)| s).collect()
}

/// R26 (step 2) / v42: an opened ability's stack by target — its per-target
/// rows' curves ranked by [`rank_curves`], live and stored.
pub fn target_stack(targets: &[SeriesRow], top: usize) -> Vec<AbilitySeries> {
    rank_curves(
        targets.iter().map(|r| {
            let mut curve = Vec::new();
            add_cells(&mut curve, &r.cells);
            (r.key.clone(), curve)
        }),
        top,
    )
}

/// v17: an opened ability's whole-fight targets as rows — `pct` of the
/// ability's own total, no rate, every row wearing the ability's school
/// and the player's class and spec, largest first (ties by label).
pub fn target_rows(
    tallies: &[TargetTally],
    school: u32,
    class: Option<Class>,
    spec: Option<Spec>,
) -> Vec<Row> {
    let total: u64 = tallies.iter().map(|t| t.amount).sum();
    let mut rows: Vec<Row> = tallies
        .iter()
        .map(|t| Row {
            key: t.target.clone(),
            label: t.target.clone(),
            amount: t.amount,
            extra: t.extra,
            count: t.count,
            crits: t.crits,
            pct: if total > 0 {
                t.amount as f64 / total as f64 * 100.0
            } else {
                0.0
            },
            class,
            spec,
            school,
            ..Row::default()
        })
        .collect();
    rows.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.label.cmp(&b.label)));
    rows
}

/// v12: a windowed comparison side's total — `rows` (a window's, from
/// [`window_rows`]) summed, `per_sec` over `secs`; the caller names whose.
pub fn total_of(rows: &[Row], secs: f64) -> Row {
    let mut total = Row::default();
    for r in rows {
        total.amount += r.amount;
        total.extra += r.extra;
        total.count += r.count;
        total.crits += r.crits;
    }
    total.per_sec = if secs > 0.0 {
        total.amount as f64 / secs
    } else {
        0.0
    };
    total
}

/// A zoom window snapped OUT to the grid — `lo` down, `hi` up to a bucket
/// edge — so what [`admits`] counts and what the rate divides by are the
/// same seconds.
pub fn snap(range: (i64, i64)) -> (i64, i64) {
    let lo = range.0.max(0) / BUCKET_MS * BUCKET_MS;
    let hi = (range.1.max(lo + 1) + BUCKET_MS - 1) / BUCKET_MS * BUCKET_MS;
    (lo, hi)
}

/// Does `bucket` overlap the window `[lo, hi)` in ms from the segment's
/// start? `None` = everything.
pub fn admits(range: Option<(i64, i64)>, bucket: u32) -> bool {
    match range {
        None => true,
        Some((lo, hi)) => {
            let b = i64::from(bucket) * BUCKET_MS;
            b + BUCKET_MS > lo && b < hi
        }
    }
}

/// `rows` summed over the seconds `range` admits (`None`: all of them),
/// as meter rows: a row with nothing in the window left out, `per_sec`
/// over `secs`, `pct` of the rows' total, every row wearing `class` and
/// `spec`, largest first (ties by label). A caller snaps the window first
/// when it wants whole seconds ([`snap`]) and passes its length as `secs`.
pub fn window_rows(
    rows: &[SeriesRow],
    range: Option<(i64, i64)>,
    secs: f64,
    class: Option<Class>,
    spec: Option<Spec>,
) -> Vec<Row> {
    let mut out: Vec<Row> = rows
        .iter()
        .filter_map(|r| {
            let mut row = Row {
                key: r.key.clone(),
                label: r.label(),
                spell_id: r.spell_id,
                school: r.school,
                class,
                spec,
                ..Row::default()
            };
            for c in r.cells.iter().filter(|c| admits(range, c.bucket)) {
                row.amount += c.amount;
                row.extra += c.extra;
                row.count += c.count;
                row.crits += c.crits;
            }
            (row.count > 0).then_some(row)
        })
        .collect();
    let total: u64 = out.iter().map(|r| r.amount).sum();
    for r in &mut out {
        r.pct = if total > 0 {
            r.amount as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        r.per_sec = if secs > 0.0 {
            r.amount as f64 / secs
        } else {
            0.0
        };
    }
    out.sort_by(|a, b| b.amount.cmp(&a.amount).then_with(|| a.label.cmp(&b.label)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(bucket: u32, amount: u64, crits: u64) -> SeriesCell {
        SeriesCell {
            bucket,
            amount,
            extra: 0,
            count: 1,
            crits,
        }
    }

    /// A window keeps the seconds it overlaps, snapped out; a pet's row
    /// wears its "(pet)" label; an empty window answers empty.
    #[test]
    fn a_window_sums_the_seconds_it_admits() {
        assert_eq!(snap((4_500, 9_600)), (4_000, 10_000));
        assert_eq!(snap((0, 0)), (0, 1_000), "never empty");
        let rows = vec![
            SeriesRow {
                key: "Fireball".into(),
                spell_id: 133,
                school: 4,
                cells: vec![cell(0, 100, 0), cell(9, 60, 1)],
            },
            SeriesRow {
                key: "Firebolt\u{0}Imp".into(),
                spell_id: 3110,
                school: 4,
                cells: vec![cell(5, 40, 0)],
            },
        ];
        let w = snap((4_500, 9_600));
        let got = window_rows(&rows, Some(w), 6.0, None, None);
        let words: Vec<(String, u64, u64)> = got
            .iter()
            .map(|r| (r.label.clone(), r.amount, r.crits))
            .collect();
        assert_eq!(
            words,
            vec![
                ("Fireball".to_string(), 60, 1),
                ("Firebolt (Imp)".to_string(), 40, 0)
            ]
        );
        assert!((got[0].pct - 60.0).abs() < 1e-9 && (got[0].per_sec - 10.0).abs() < 1e-9);
        assert_eq!(got[0].school, 4);
        assert!(window_rows(&rows, Some((20_000, 30_000)), 10.0, None, None).is_empty());
        assert_eq!(window_rows(&rows, None, 10.0, None, None).len(), 2);
    }
}
