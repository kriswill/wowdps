//! R21: the stack ledger as matrices (layout D of the design study) — rows
//! the abilities that hit the player, columns the open debuff's stack
//! level, cells the average hit — and the heat a cell's average wears
//! against its ROW, because the question is "how much worse per stack",
//! not "which ability hits hardest". Moved from the iced window's
//! `taken.rs`; both window GUIs draw from it.

use wowdps_model::{StackBase, StackCell, StackingDebuff};

use crate::theme::{Color, WindowTokens};

/// One debuff's matrix, derived. Rows are the abilities that hit under it,
/// columns the levels 0..=max; a cell is (hits, average) or `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    pub aura: String,
    pub aura_spell_id: u32,
    pub max_level: u16,
    pub rows: Vec<MatrixRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixRow {
    pub label: String,
    /// Index 0 is the DERIVED level 0 (R21: the by-ability baseline minus
    /// every cell of that spell), then one per level 1..=max.
    pub cells: Vec<Option<(u32, u64)>>,
}

impl MatrixRow {
    /// The row's averages' range, for its heat: (smallest, largest).
    pub fn range(&self) -> (u64, u64) {
        let avgs = self.cells.iter().flatten().map(|(_, avg)| *avg);
        (avgs.clone().min().unwrap_or(0), avgs.max().unwrap_or(0))
    }

    /// Every hit the row counts, over its levels.
    pub fn hits(&self) -> u32 {
        self.cells.iter().flatten().map(|(h, _)| *h).sum()
    }

    /// Where `avg` sits in the row's range, 0 at its smallest (0 when
    /// every level averages the same).
    pub fn heat_t(&self, avg: u64) -> f32 {
        let (lo, hi) = self.range();
        if hi > lo {
            (avg - lo) as f32 / (hi - lo) as f32
        } else {
            0.0
        }
    }
}

/// Fold the ledger into one matrix per debuff. Level 0 is the reader's
/// derivation, per damage spell id and PER DEBUFF: `base − Σ this debuff's
/// cells` — every hit of the spell landed either under this debuff (in
/// exactly one of its cells) or not, so the remainder is exactly its
/// level 0, and a second debuff open at the same time changes nothing.
/// Clamped at nothing rather than negative; a spell with no baseline (a
/// pre-retest snapshot) leaves level 0 empty rather than inventing it.
pub fn matrices(
    stacking: &[StackingDebuff],
    cells: &[StackCell],
    base: &[StackBase],
) -> Vec<Matrix> {
    stacking
        .iter()
        .map(|d| {
            let mut labels: Vec<(u32, String)> = Vec::new();
            for c in cells.iter().filter(|c| c.aura_spell_id == d.spell_id) {
                if !labels.iter().any(|(id, _)| *id == c.damage_spell_id) {
                    labels.push((c.damage_spell_id, c.damage_label.clone()));
                }
            }
            let rows = labels
                .into_iter()
                .map(|(id, label)| {
                    let mut out: Vec<Option<(u32, u64)>> = vec![None; d.max_level as usize + 1];
                    for c in cells
                        .iter()
                        .filter(|c| c.aura_spell_id == d.spell_id && c.damage_spell_id == id)
                    {
                        if let Some(slot) = out.get_mut(c.level as usize)
                            && c.hits > 0
                        {
                            *slot = Some((c.hits, c.sum / u64::from(c.hits)));
                        }
                    }
                    // Level 0: the unconditioned baseline less this debuff's
                    // own cells for the spell.
                    if let Some(b) = base.iter().find(|b| b.damage_spell_id == id) {
                        let (ch, cs) = cells
                            .iter()
                            .filter(|c| c.damage_spell_id == id && c.aura_spell_id == d.spell_id)
                            .fold((0u32, 0u64), |(h, s), c| (h + c.hits, s + c.sum));
                        let hits = b.hits.saturating_sub(ch);
                        let sum = b.sum.saturating_sub(cs);
                        if hits > 0
                            && let Some(slot) = out.first_mut()
                        {
                            *slot = Some((hits, sum / u64::from(hits)));
                        }
                    }
                    MatrixRow { label, cells: out }
                })
                .collect();
            Matrix {
                aura: d.label.clone(),
                aura_spell_id: d.spell_id,
                max_level: d.max_level,
                rows,
            }
        })
        .filter(|m| !m.rows.is_empty())
        .collect()
}

/// Heat between the theme's good (a row's smallest average) and its bad
/// (its largest), through amber: one colour per cell, a straight line
/// through each half.
pub fn heat(t: f32, w: &WindowTokens) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (a, b, s) = if t < 0.5 {
        (w.good, w.amber, t * 2.0)
    } else {
        (w.amber, w.bad, (t - 0.5) * 2.0)
    };
    Color::rgba(
        a.r + (b.r - a.r) * s,
        a.g + (b.g - a.g) * s,
        a.b + (b.b - a.b) * s,
        1.0,
    )
}

/// What the foot under the matrices says: how level 0 is had, and — when
/// the ledger dropped hits past its cap — how many.
pub const FOOT: &str =
    "level 0 is derived from the by-ability row; a hit under two debuffs counts in both";

pub fn dropped_words(dropped: u32) -> Option<String> {
    (dropped > 0).then(|| format!("{dropped} hits past the cell cap"))
}

/// The ledger the matrices' tests build on, in every crate that draws one.
#[doc(hidden)]
pub mod samples {
    use wowdps_model::{StackBase, StackCell, StackingDebuff};

    /// A tank's Crushing Smash ladder: Tectonic Strike's hits at stacks 1
    /// to 3 over a 10-hit baseline.
    pub fn ledger() -> (Vec<StackingDebuff>, Vec<StackCell>, Vec<StackBase>) {
        let debuff = StackingDebuff {
            spell_id: 100,
            label: "Crushing Smash".into(),
            src: "Boss".into(),
            max_level: 3,
            hits: 6,
        };
        let cell = |level: u16, hits: u32, sum: u64| StackCell {
            damage_spell_id: 7,
            damage_label: "Tectonic Strike".into(),
            aura_spell_id: 100,
            level,
            hits,
            sum,
            max: sum,
        };
        let cells = vec![cell(1, 2, 400), cell(2, 2, 600), cell(3, 2, 1_000)];
        let base = vec![StackBase {
            damage_spell_id: 7,
            damage_label: "Tectonic Strike".into(),
            hits: 10,
            sum: 2_400,
            misses: 1,
        }];
        (vec![debuff], cells, base)
    }
}

#[cfg(test)]
mod tests {
    use super::samples::ledger;
    use super::*;
    use crate::theme::GOLD;

    #[test]
    fn the_matrix_derives_level_zero_and_orders_the_levels() {
        let (d, c, b) = ledger();
        let m = matrices(&d, &c, &b);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].aura, "Crushing Smash");
        assert_eq!(m[0].rows.len(), 1);
        let r = &m[0].rows[0];
        assert_eq!(r.label, "Tectonic Strike");
        // 10 hits / 2 400 total, 6 hits / 2 000 in cells → level 0 is
        // 4 hits averaging 100.
        assert_eq!(
            r.cells,
            vec![
                Some((4, 100)),
                Some((2, 200)),
                Some((2, 300)),
                Some((2, 500))
            ]
        );
        assert_eq!((r.range(), r.hits()), ((100, 500), 10));
        assert_eq!(r.heat_t(300), 0.5);
        // Without a baseline level 0 stays honestly empty.
        let m = matrices(&d, &c, &[]);
        assert_eq!(m[0].rows[0].cells[0], None);
        // Cells that outnumber the baseline clamp at nothing.
        let mut low = b.clone();
        low[0].hits = 3;
        low[0].sum = 100;
        let m = matrices(&d, &c, &low);
        assert_eq!(m[0].rows[0].cells[0], None);
        // A debuff nothing landed under draws no matrix.
        assert!(matrices(&d, &[], &b).is_empty());
        // A second debuff open over the same hits takes nothing from the
        // first one's level 0: each debuff's remainder is its own.
        let mut two = d.clone();
        two.push(StackingDebuff {
            spell_id: 200,
            label: "Rending Slash".into(),
            src: "Boss".into(),
            max_level: 1,
            hits: 6,
        });
        let mut overlapped = c.clone();
        overlapped.push(StackCell {
            damage_spell_id: 7,
            damage_label: "Tectonic Strike".into(),
            aura_spell_id: 200,
            level: 1,
            hits: 6,
            sum: 2_000,
            max: 500,
        });
        let m = matrices(&two, &overlapped, &b);
        assert_eq!(m.len(), 2);
        assert_eq!(
            m[0].rows[0].cells[0],
            Some((4, 100)),
            "unchanged by the overlap"
        );
        assert_eq!(m[1].rows[0].cells, vec![Some((4, 100)), Some((6, 333))]);
    }

    #[test]
    fn heat_runs_good_through_amber_to_bad() {
        let w = GOLD.window;
        assert_eq!(heat(0.0, &w), w.good);
        assert_eq!(heat(1.0, &w), w.bad);
        let mid = heat(0.5, &w);
        assert!((mid.r - w.amber.r).abs() < 0.01 && (mid.g - w.amber.g).abs() < 0.01);
        assert_eq!(dropped_words(0), None);
        assert_eq!(
            dropped_words(3).as_deref(),
            Some("3 hits past the cell cap")
        );
    }
}
