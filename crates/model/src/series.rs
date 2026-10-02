//! v39: a fight's abilities (and targets) second by second, and the ONE
//! definition of what a zoom window makes of them. The live meter folds
//! its sparse per-spell series into [`SeriesRow`]s and windows them here
//! (`Segment::spells_in`, `damage_targets_in`); the history store keeps the
//! same rows in its series tier and windows them here too — so a stored
//! pull's zoom answers exactly what the live one did.

use crate::{Class, Row, Spec};

/// The grid every series is bucketed on (R12's): one second.
pub const BUCKET_MS: i64 = 1_000;

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
