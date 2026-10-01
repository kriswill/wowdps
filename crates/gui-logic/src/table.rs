//! What a table's columns mean: a numeric column's heading, its cell's text
//! and the value a sort orders it by, and the drawn order a sort gives a
//! list of `Row`s. Every one is derivable from a `Row` alone. How wide a
//! column is drawn and in which ink is each GUI's (the iced GUI's
//! `table::ColDraw`).

use wowdps_model::fmt::{commas, human};
use wowdps_model::{Row, View};

/// A numeric column. Every one is derivable from a `Row` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Col {
    Amount,
    /// Per second.
    Rate,
    /// Share of the list's total.
    Pct,
    /// Crit rate over the contributing events, in whole percents (a
    /// drill's).
    Crit,
    /// The live meter's crit rate, to a tenth (`pct(r.x)`: "35.2%").
    CritFine,
    /// Contributing events: hits/ticks, heals, or the recorded count.
    Hits,
    /// Average per event.
    Avg,
    /// Healing: the share of healing that landed as overheal, `extra`
    /// over `amount + extra`.
    Overheal,
    /// Taken (R17): what absorbs took off the hits, `extra` in full.
    Absorbed,
}

/// A view whose rows are counts, not amounts: no rate, crit, hits or
/// average means anything there.
pub fn counted(view: View) -> bool {
    matches!(
        view,
        View::Interrupts | View::CrowdControl | View::Dispels | View::Deaths
    )
}

impl Col {
    /// The heading in a view, in the prototype's words and its sentence
    /// case (`Amount`, `Per sec`, `Share`, `Crit`). `""` means the column
    /// is meaningless there: it is drawn blank and takes no click.
    pub fn head(self, view: View) -> &'static str {
        match self {
            // What was taken is an amount like any other: the view's tab
            // already says whose (the prototype's heads say "Amount").
            Col::Amount => match view {
                v if counted(v) => "Count",
                _ => "Amount",
            },
            // The cells carry their own "%".
            Col::Overheal => "Overheal",
            Col::Absorbed => "Absorbed",
            Col::Rate => {
                if counted(view) {
                    ""
                } else {
                    "Per sec"
                }
            }
            Col::Pct => "Share",
            Col::Crit | Col::CritFine => {
                if counted(view) {
                    ""
                } else {
                    "Crit"
                }
            }
            Col::Hits => {
                if counted(view) {
                    ""
                } else {
                    "Hits"
                }
            }
            Col::Avg => {
                if counted(view) {
                    ""
                } else {
                    "Avg"
                }
            }
        }
    }

    /// The cell's text; blank where the row has nothing to say (no
    /// overkill, no crits, no events) rather than a misleading 0.
    pub fn cell(self, r: &Row) -> String {
        match self {
            Col::Amount => figure(r.amount),
            Col::Rate if r.per_sec >= 1.0 => figure(r.per_sec as u64),
            Col::Pct => format!("{:>4.1}%", r.pct),
            Col::Crit if r.crits > 0 => format!("{:.0}%", r.crit_pct()),
            Col::CritFine if r.crits > 0 => format!("{:.1}%", r.crit_pct()),
            Col::Hits if r.count > 0 => commas(r.count),
            Col::Avg if r.count > 0 => figure(r.amount / r.count),
            // A heal with none wasted is a measured 0%; no healing at all
            // is nothing to say.
            Col::Overheal if r.amount + r.extra > 0 => format!("{:.0}%", overheal_pct(r)),
            Col::Absorbed if r.extra > 0 => figure(r.extra),
            _ => String::new(),
        }
    }

    /// The value a sort on this column orders by.
    pub fn key(self, r: &Row) -> f64 {
        match self {
            Col::Amount => r.amount as f64,
            Col::Rate => r.per_sec,
            Col::Pct => r.pct,
            Col::Crit | Col::CritFine => r.crit_pct(),
            Col::Hits => r.count as f64,
            Col::Avg => {
                if r.count > 0 {
                    r.amount as f64 / r.count as f64
                } else {
                    0.0
                }
            }
            Col::Overheal => overheal_pct(r),
            Col::Absorbed => r.extra as f64,
        }
    }
}

/// Healing's overheal share, 0..100: what landed on full health against
/// everything cast (`extra` over `amount + extra`). 0 with no healing.
pub fn overheal_pct(r: &Row) -> f64 {
    let cast = r.amount + r.extra;
    if cast == 0 {
        0.0
    } else {
        r.extra as f64 / cast as f64 * 100.0
    }
}

/// A figure as the window's tables write it: `human`'s one decimal up to
/// the millions, and two at a billion and over — the prototype's `fC`
/// ("1.49B"), where a raid's total sits beside rows of "92.7M" and a stat
/// line of 1,488,795,375, and "1.5B" would be the one figure a reader
/// could not reconcile with them. `human` itself is the overlay's too, and
/// stays as it is.
pub fn figure(n: u64) -> String {
    // Where `human` would round up into the billions, from 999.95 M.
    if n >= 999_950_000 {
        format!("{:.2}B", n as f64 / 1e9)
    } else {
        human(n)
    }
}

/// `rows` in the drawn order: stable-sorted by `sort` when there is one,
/// each with the index it arrived with — the index a click sends back and
/// the rank a row keeps. `None` is the order the rows came in.
pub fn sorted(mut rows: Vec<(usize, Row)>, sort: Option<(Col, bool)>) -> Vec<(usize, Row)> {
    if let Some((col, desc)) = sort {
        rows.sort_by(|(_, a), (_, b)| {
            let o = col
                .key(a)
                .partial_cmp(&col.key(b))
                .unwrap_or(std::cmp::Ordering::Equal);
            if desc { o.reverse() } else { o }
        });
    }
    rows
}

/// An ability's label split into the ability and the pet or guardian that
/// cast it: "Fel Firebolt (Wild Imp)" → ("Fel Firebolt", "Wild Imp").
pub fn split_pet(label: &str) -> (&str, Option<&str>) {
    match label
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
    {
        Some((name, pet)) if !name.is_empty() && !pet.is_empty() => (name, Some(pet)),
        _ => (label, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(label: &str, amount: u64, count: u64, crits: u64) -> Row {
        Row {
            key: label.to_string(),
            label: label.to_string(),
            amount,
            extra: 0,
            count,
            crits,
            per_sec: amount as f64 / 10.0,
            pct: 50.0,
            class: None,
            spec: None,
            hp: None,
            gain: false,
            spell_id: 0,
            enemy: false,
            school: 0,
            mine: false,
            offset_ms: None,
        }
    }

    #[test]
    fn cells_stay_blank_where_a_row_has_nothing_to_say() {
        let r = row("Melee", 0, 0, 0);
        assert_eq!(Col::Crit.cell(&r), "");
        assert_eq!(Col::Hits.cell(&r), "");
        assert_eq!(Col::Avg.cell(&r), "");
        assert_eq!(Col::Rate.cell(&r), "");
        let r = row("Pyroblast", 1_000, 4, 1);
        assert_eq!(Col::Hits.cell(&r), "4");
        assert_eq!(Col::Avg.cell(&r), "250");
        assert_eq!(Col::Crit.cell(&r), "25%");
    }

    #[test]
    fn headings_follow_the_view_and_count_views_blank_the_rates() {
        assert_eq!(Col::Rate.head(View::Damage), "Per sec");
        assert_eq!(Col::Rate.head(View::Healing), "Per sec");
        assert_eq!(Col::Rate.head(View::Taken), "Per sec");
        assert_eq!(Col::Rate.head(View::Interrupts), "");
        assert_eq!(Col::Amount.head(View::Damage), "Amount");
        assert_eq!(Col::Amount.head(View::Dispels), "Count");
        assert_eq!(Col::Amount.head(View::Taken), "Amount");
        assert_eq!(Col::Pct.head(View::Damage), "Share");
        assert_eq!(Col::Crit.head(View::Deaths), "");
        assert_eq!(Col::Overheal.head(View::Healing), "Overheal");
        assert_eq!(Col::CritFine.head(View::EnemyTaken), "Crit");
        assert_eq!(Col::Absorbed.head(View::Taken), "Absorbed");
    }

    /// Hits are counted with their thousands marked, as the prototype's
    /// `fN` writes them ("2,140").
    #[test]
    fn hits_carry_their_commas() {
        let r = Row {
            count: 2_140,
            ..Row::default()
        };
        assert_eq!(Col::Hits.cell(&r), "2,140");
    }

    #[test]
    fn sorting_keeps_every_index_and_none_is_arrival_order() {
        let rows: Vec<(usize, Row)> = [("a", 5, 5, 5), ("b", 9, 3, 0), ("c", 1, 1, 1)]
            .iter()
            .enumerate()
            .map(|(i, (l, a, n, c))| (i, row(l, *a, *n, *c)))
            .collect();
        let by_amount = sorted(rows.clone(), Some((Col::Amount, true)));
        assert_eq!(
            by_amount.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![1, 0, 2]
        );
        let by_crit_asc = sorted(rows.clone(), Some((Col::Crit, false)));
        assert_eq!(
            by_crit_asc.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![1, 0, 2],
            "b has no crits, a is 100%, c is 100% — stable keeps a before c"
        );
        let plain = sorted(rows, None);
        assert_eq!(
            plain.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn a_billion_reads_to_two_places_and_less_as_before() {
        assert_eq!(figure(1_488_795_375), "1.49B");
        assert_eq!(figure(1_455_000_000), "1.46B");
        assert_eq!(figure(999_960_000), "1.00B");
        assert_eq!(figure(92_700_000), human(92_700_000));
        assert_eq!(figure(219_700), "219.7k");
        assert_eq!(figure(512), "512");
        let mut r = row("Shadow Bolt", 100, 3, 1);
        assert_eq!(Col::CritFine.cell(&r), "33.3%");
        assert_eq!(Col::Crit.cell(&r), "33%", "a drill reads whole percents");
        r.crits = 0;
        assert_eq!(Col::CritFine.cell(&r), "");
    }

    /// A pet's name comes off the end of the label; a label that only
    /// looks like it keeps its words.
    #[test]
    fn a_pet_s_name_splits_off_the_ability() {
        assert_eq!(
            split_pet("Fel Firebolt (Wild Imp)"),
            ("Fel Firebolt", Some("Wild Imp"))
        );
        assert_eq!(split_pet("Shadow Bolt"), ("Shadow Bolt", None));
        assert_eq!(split_pet("(Odd)"), ("(Odd)", None));
        assert_eq!(
            split_pet("Soul Barrage (Antoran Jailer)"),
            ("Soul Barrage", Some("Antoran Jailer"))
        );
    }
}
