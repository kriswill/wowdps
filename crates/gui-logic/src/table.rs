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

/// R13: where the enemy team's block starts — the first `enemy` row, but
/// only when the teams are contiguous (sorted views group them; the Deaths
/// view is in death order and stays mixed, so it draws no divider).
pub fn enemy_split(rows: &[wowdps_model::Row]) -> Option<usize> {
    let split = rows.iter().position(|r| r.enemy)?;
    rows.iter().skip(split).all(|r| r.enemy).then_some(split)
}

// ---- the row filter -----------------------------------------------------------

/// Does this row answer to `needle` (already folded — see [`crate::fold`])?
///
/// Substring, so "prot" finds Protection and "resto" finds Restoration
/// without an abbreviation table, and "protection" legitimately matches both
/// Protection Warrior and Protection Paladin — the class name is how a
/// reader narrows that, not a bug. A row whose class or spec R8 has not
/// inferred yet answers to neither term; it is not matched by everything,
/// and an empty filter still keeps it.
///
/// Every field goes through the SAME folding comparison, so an accented
/// class or spec name folds exactly as a player name does and the two paths
/// cannot drift.
fn row_matches(r: &Row, needle: &[char]) -> bool {
    use crate::fold;
    fold::contains(&r.label, needle)
        || r.class.is_some_and(|c| fold::contains(c.name(), needle))
        || r.spec.is_some_and(|s| fold::contains(s.name(), needle))
        || r.spec
            .is_some_and(|s| fold::contains(s.role().name(), needle))
}

/// Accent-folded, case-insensitive substring over what a row IS — its
/// label, its class, its spec and its role, so "akanos" finds `Akanôs` —
/// keeping each row's position in the UNFILTERED list. The index is both
/// the rank the row displays and the one a click sends back to
/// `ClientState`, so it must survive filtering or a filtered click would
/// drill into the wrong player. Ranks and shares are never recomputed.
pub fn filtered_indexed(rows: Vec<Row>, filter: &str) -> Vec<(usize, Row)> {
    // Folded ONCE per call, not once per row: this runs on every snapshot at
    // 10 Hz over a whole raid's rows.
    let needle = crate::fold::fold(filter);
    rows.into_iter()
        .enumerate()
        .filter(|(_, r)| needle.is_empty() || row_matches(r, &needle))
        .collect()
}

/// The rows as DRAWN: filtered, then sorted by the chosen column, each with
/// the index the daemon gave it — the index a click sends back and the
/// rank a row keeps. Ranks and shares are never recomputed: sorting by crit
/// asks a different question of the same chart, it does not make a new one.
pub fn ordered(rows: Vec<Row>, filter: &str, sort: Option<(Col, bool)>) -> Vec<(usize, Row)> {
    sorted(filtered_indexed(rows, filter), sort)
}

// ---- the grid -------------------------------------------------------------------

/// Gap between an inspector list's columns, and between the headings over
/// them.
pub const GAP: f32 = 10.0;
/// The live meter's gap (`.thead, .trow, .ttotal{column-gap:12px}`).
pub const METER_GAP: f32 = 12.0;
/// The heading line's own inset each side: a list whose rows stand `side`
/// from its edges pads its heading line `side` less this.
pub const HEADS_INSET: f32 = 8.0;
/// The total row's inset each side (`.ttotal{padding:0 8px}`) — what a
/// caller lining its label up with a column above subtracts.
pub const TOTAL_INSET: f32 = 8.0;
/// The inspector's heading line (`.ihrow{font-size:13px}`).
pub const INSPECTOR_HEAD_PX: f32 = 13.0;

impl Col {
    /// Each column's width at the prototype's 14.5 px figures: its widest
    /// cell ("100.0%", "12345") with a step of air.
    pub fn width(self) -> f32 {
        match self {
            Col::Amount => 62.0,
            Col::Rate => 58.0,
            Col::Pct => 52.0,
            Col::Crit => 44.0,
            Col::CritFine => 50.0,
            Col::Hits => 50.0,
            Col::Avg => 56.0,
            // "Overheal" and "Absorbed", the headings, are what set these.
            Col::Overheal => 62.0,
            Col::Absorbed => 62.0,
        }
    }

    /// How much a column's figure matters: the amount is the number every
    /// eye goes to (0); the rate is second (1); everything else — the
    /// share, the counts, crit, the overheal and the absorbed — is
    /// secondary (2), drawn in the secondary ink.
    pub fn rank(self) -> u8 {
        match self {
            Col::Amount => 0,
            Col::Rate => 1,
            Col::Pct
            | Col::Hits
            | Col::Avg
            | Col::Crit
            | Col::CritFine
            | Col::Overheal
            | Col::Absorbed => 2,
        }
    }
}

/// Every column, for the tests that hold a rule over all of them.
pub const ALL_COLS: [Col; 9] = [
    Col::Amount,
    Col::Rate,
    Col::Pct,
    Col::Crit,
    Col::CritFine,
    Col::Hits,
    Col::Avg,
    Col::Overheal,
    Col::Absorbed,
];

/// The widths and the gap a table's columns are drawn at: the live meter's
/// — and the enemy drill's attackers, which wear its row — the prototype's
/// own grid for its view (`.v-num4`, `.v-enemy`, `.v-count`), narrowed as
/// it narrows under 820 px; the inspector's lists theirs (`.t-ab`, `.t-tg`,
/// `.cmp2`) at [`GAP`], a column they do not set keeping [`Col::width`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grid {
    Meter {
        view: View,
        narrow: bool,
    },
    /// The inspector's ability list (`.t-ab{--icols:… 60px 46px 46px 42px
    /// 56px}`), and in an inspector 440 px or narrower its three (`… 58px
    /// 46px 42px`).
    Abilities {
        narrow: bool,
    },
    /// The inspector's target list (`.t-tg{--icols:… 64px 50px 50px}`).
    Targets,
    /// A comparison's two lists side by side (`.cmp2 .irow{… 50px 40px}`).
    Pair,
}

impl Grid {
    /// A column's width on this grid.
    pub fn width(self, c: Col) -> f32 {
        let (view, narrow) = match self {
            Grid::Abilities { narrow } => {
                return match c {
                    Col::Amount if narrow => 58.0,
                    Col::Amount => 60.0,
                    Col::Pct | Col::Hits => 46.0,
                    Col::Crit => 42.0,
                    Col::Avg => 56.0,
                    _ => c.width(),
                };
            }
            Grid::Targets => {
                return match c {
                    Col::Amount => 64.0,
                    Col::Pct | Col::Hits => 50.0,
                    _ => c.width(),
                };
            }
            Grid::Pair => {
                return match c {
                    Col::Amount => 50.0,
                    Col::Pct => 40.0,
                    _ => c.width(),
                };
            }
            Grid::Meter { view, narrow } => (view, narrow),
        };
        match (view, c) {
            // `.v-count{--cols:30px minmax(0,1fr) 72px 72px}`, at any width.
            (v, Col::Amount | Col::Pct) if counted(v) => 72.0,
            // `.v-num4, .v-enemy{--cols:26px minmax(0,1fr) 62px 62px}`.
            (_, Col::Amount | Col::Rate) if narrow => 62.0,
            // `.v-enemy{--cols:30px minmax(0,1fr) 72px 72px 52px 52px}`.
            (View::EnemyTaken, Col::Amount | Col::Rate) => 72.0,
            (View::EnemyTaken, Col::Pct | Col::CritFine) => 52.0,
            // `.v-num4{--cols:30px minmax(0,1fr) 68px 66px 52px 58px}`.
            (_, Col::Amount) => 68.0,
            (_, Col::Rate) => 66.0,
            (_, Col::Pct) => 52.0,
            (_, Col::CritFine | Col::Overheal | Col::Absorbed) => 58.0,
            _ => c.width(),
        }
    }

    /// Is this one of the inspector's grids (`.ilist`)?
    pub fn inspector(self) -> bool {
        matches!(self, Grid::Abilities { .. } | Grid::Targets | Grid::Pair)
    }

    /// The heading line's size on this grid: the meter's `.thead` at the
    /// label size, the inspector's `.ihrow{font-size:13px}` — one size for
    /// the whole line, the lead's included.
    pub fn head_px(self) -> f32 {
        if self.inspector() {
            INSPECTOR_HEAD_PX
        } else {
            crate::theme::SIZES.label
        }
    }

    /// The gap between this grid's columns.
    pub fn gap(self) -> f32 {
        match self {
            Grid::Abilities { .. } | Grid::Targets | Grid::Pair => GAP,
            Grid::Meter { .. } => METER_GAP,
        }
    }

    /// Width the numeric block claims on this grid: every column plus the
    /// gaps between them.
    pub fn span(self, cols: &[Col], scale: f32) -> f32 {
        let widths: f32 = cols.iter().map(|&c| self.width(c)).sum();
        (widths + (cols.len().saturating_sub(1)) as f32 * self.gap()) * scale
    }
}

/// The meter in a narrow window: the prototype's two, the amount and the
/// rate (`.v-num4 .c5, .c6 {display:none}` at 820 px).
pub const METER_NARROW: &[Col] = &[Col::Amount, Col::Rate];
/// The live meter's four figures on Damage and Enemies (`.v-num4`,
/// `.v-enemy`): amount, rate, share, crit.
pub const METER_DAMAGE: &[Col] = &[Col::Amount, Col::Rate, Col::Pct, Col::CritFine];
/// Healing's: the fourth is the overheal share.
pub const METER_HEALING: &[Col] = &[Col::Amount, Col::Rate, Col::Pct, Col::Overheal];
/// Taken's (R17): the fourth is what absorbs took.
pub const METER_TAKEN: &[Col] = &[Col::Amount, Col::Rate, Col::Pct, Col::Absorbed];
/// A count view's two (`.v-count`), at every width: the count and its
/// share — a count has no rate, and blank columns are width a name needs.
pub const METER_COUNT: &[Col] = &[Col::Amount, Col::Pct];

/// The live meter's columns for `view` (the prototype's `VIEWS.heads`):
/// the amount, the rate, the share and the one figure the view is read
/// by after them — crit, the overheal share, what absorbs took. A narrow
/// window keeps the amount and the rate; a count view its count and share.
pub fn meter_set(view: View, narrow: bool) -> &'static [Col] {
    match view {
        v if counted(v) => METER_COUNT,
        _ if narrow => METER_NARROW,
        View::Healing => METER_HEALING,
        View::Taken => METER_TAKEN,
        _ => METER_DAMAGE,
    }
}

/// How `c` is sorted under `sort`: `Some(true)` descending, `Some(false)`
/// ascending, `None` not at all.
pub fn sort_of(c: Col, sort: Option<(Col, bool)>) -> Option<bool> {
    sort.and_then(|(s, desc)| (s == c).then_some(desc))
}

/// The pinned total's words in `cols`: the fold of `rows` — the amount and
/// the rate, the share "100%" (blank while a filter narrows the rows,
/// `filtered`, whose share of the chart the total is not) — and the view's
/// fourth figure (crit, overheal, absorbed) left blank, a rate of a sum
/// being no figure a reader reads there.
pub fn total_cells(cols: &[Col], rows: &[Row], filtered: bool) -> Vec<String> {
    let sum = Row {
        amount: rows.iter().map(|r| r.amount).sum(),
        extra: rows.iter().map(|r| r.extra).sum(),
        count: rows.iter().map(|r| r.count).sum(),
        crits: rows.iter().map(|r| r.crits).sum(),
        per_sec: rows.iter().map(|r| r.per_sec).sum(),
        pct: 100.0,
        ..Row::default()
    };
    cols.iter()
        .map(|c| match c {
            Col::Pct if filtered => String::new(),
            Col::Pct => "100%".to_string(),
            Col::Crit | Col::CritFine | Col::Overheal | Col::Absorbed => String::new(),
            _ => c.cell(&sum),
        })
        .collect()
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

    /// The live meter's columns are the prototype's per view: amount, rate
    /// and share, then crit, the overheal share or what absorbs took — no
    /// overkill column — and a count view's count and share at any width.
    #[test]
    fn the_live_meter_reads_each_view_by_its_own_fourth_figure() {
        let four = |v| meter_set(v, false);
        assert_eq!(four(View::Damage), METER_DAMAGE);
        assert_eq!(four(View::EnemyTaken), METER_DAMAGE);
        assert_eq!(four(View::Healing).last(), Some(&Col::Overheal));
        assert_eq!(four(View::Taken).last(), Some(&Col::Absorbed));
        for v in View::ALL {
            assert!(four(v).starts_with(&[Col::Amount]), "{v:?}");
        }
        for v in [View::Interrupts, View::Deaths] {
            assert_eq!(meter_set(v, false), METER_COUNT);
            assert_eq!(meter_set(v, true), METER_COUNT, "a count keeps its share");
        }
        assert_eq!(meter_set(View::Healing, true), METER_NARROW);
        assert_eq!(METER_NARROW, &[Col::Amount, Col::Rate]);
        // The overheal share is of everything cast; the absorbed figure
        // is the extra in full, and neither says a misleading 0.
        let mut heal = row("Flash Heal", 300, 3, 0);
        heal.extra = 100;
        assert_eq!(Col::Overheal.cell(&heal), "25%");
        assert_eq!(Col::Overheal.key(&heal), 25.0);
        heal.extra = 0;
        assert_eq!(Col::Overheal.cell(&heal), "0%", "none wasted is measured");
        assert_eq!(Col::Overheal.cell(&row("idle", 0, 0, 0)), "");
        let mut hit = row("Crushing Smash", 5_000, 1, 0);
        assert_eq!(Col::Absorbed.cell(&hit), "");
        hit.extra = 1_500;
        assert_eq!(Col::Absorbed.cell(&hit), "1.5k");
    }

    #[test]
    fn the_span_is_the_columns_plus_the_gaps_between_them() {
        let (a, p) = (64.0, 50.0);
        let span = |cols: &[Col], scale| Grid::Targets.span(cols, scale);
        assert_eq!(span(&[Col::Amount], 1.0), a);
        assert_eq!(span(&[Col::Amount, Col::Pct], 1.0), a + p + GAP);
        assert_eq!(span(&[Col::Amount, Col::Pct], 2.0), 2.0 * (a + p + GAP));
        assert_eq!(span(&[], 1.0), 0.0);
    }

    /// The inspector's lists stand on the prototype's own columns: the
    /// abilities' `.t-ab` (60 46 46 42 56, and 58 for the amount in an
    /// inspector 440 px or narrower), the targets' `.t-tg` (64 50 50) and
    /// a comparison's `.cmp2` (50 40), all 10 px apart. The inspector's
    /// heading line is one 13 px size, the meter's the label's 13.5.
    #[test]
    fn the_inspector_s_lists_stand_on_the_prototype_s_columns() {
        let widths = |g: Grid, cols: &[Col]| cols.iter().map(|&c| g.width(c)).collect::<Vec<_>>();
        let abilities = [Col::Amount, Col::Pct, Col::Hits, Col::Crit, Col::Avg];
        assert_eq!(
            widths(Grid::Abilities { narrow: false }, &abilities),
            [60.0, 46.0, 46.0, 42.0, 56.0]
        );
        assert_eq!(
            widths(
                Grid::Abilities { narrow: true },
                &[Col::Amount, Col::Pct, Col::Crit]
            ),
            [58.0, 46.0, 42.0]
        );
        assert_eq!(
            widths(Grid::Targets, &[Col::Amount, Col::Pct, Col::Hits]),
            [64.0, 50.0, 50.0]
        );
        assert_eq!(widths(Grid::Pair, &[Col::Amount, Col::Pct]), [50.0, 40.0]);
        for g in [Grid::Abilities { narrow: false }, Grid::Targets, Grid::Pair] {
            assert_eq!(g.gap(), GAP);
            assert_eq!(g.head_px(), 13.0, "{g:?}");
        }
        let meter = Grid::Meter {
            view: View::Damage,
            narrow: false,
        };
        assert_eq!(meter.head_px(), crate::theme::SIZES.label);
    }

    /// The live meter stands on the prototype's own grid, per view, at its
    /// 12 px gap — `.v-num4` 68 / 66 / 52 / 58, `.v-enemy` 72 / 72 / 52 /
    /// 52, `.v-count` 72 / 72, and 62 / 62 under 820 px — so its right
    /// edges step 70, 64 and 78 from the last, as the reference's do. Every
    /// other table keeps its own widths and gap.
    #[test]
    fn the_live_meter_stands_on_the_prototype_s_grid() {
        let grid = |view, narrow| Grid::Meter { view, narrow };
        let widths = |g: Grid, cols: &[Col]| cols.iter().map(|&c| g.width(c)).collect::<Vec<_>>();
        let d = grid(View::Damage, false);
        assert_eq!(widths(d, METER_DAMAGE), [68.0, 66.0, 52.0, 58.0]);
        assert_eq!(
            widths(grid(View::Healing, false), METER_HEALING),
            [68.0, 66.0, 52.0, 58.0]
        );
        assert_eq!(
            widths(grid(View::Taken, false), METER_TAKEN),
            [68.0, 66.0, 52.0, 58.0]
        );
        assert_eq!(
            widths(grid(View::EnemyTaken, false), METER_DAMAGE),
            [72.0, 72.0, 52.0, 52.0]
        );
        assert_eq!(
            widths(grid(View::Interrupts, false), METER_COUNT),
            [72.0, 72.0]
        );
        assert_eq!(
            widths(grid(View::Deaths, true), METER_COUNT),
            [72.0, 72.0],
            "a count keeps its grid at any width"
        );
        assert_eq!(widths(grid(View::Damage, true), METER_NARROW), [62.0, 62.0]);
        assert_eq!(d.gap(), 12.0);
        // Right edges, from the last column's: each step is the column's
        // width and the gap before it.
        let steps: Vec<f32> = METER_DAMAGE[1..]
            .iter()
            .rev()
            .map(|&c| d.width(c) + d.gap())
            .collect();
        assert_eq!(steps, [70.0, 64.0, 78.0]);
        assert_eq!(d.span(METER_DAMAGE, 1.0), 68.0 + 66.0 + 52.0 + 58.0 + 36.0);
        // The inspector's grids set their own columns; one they do not set
        // keeps its own width.
        for g in [Grid::Abilities { narrow: false }, Grid::Targets, Grid::Pair] {
            assert_eq!(g.width(Col::Rate), Col::Rate.width(), "{g:?}");
        }
    }

    /// The pinned total folds the rows: amount and rate summed, the share
    /// "100%" — blank under a filter — and the fourth figure blank.
    #[test]
    fn the_total_folds_the_rows_and_leaves_the_fourth_figure_blank() {
        let mut rows = vec![row("a", 1_000_000_000, 10, 5), row("b", 488_795_375, 10, 1)];
        rows[0].per_sec = 2_369_000.0;
        rows[1].per_sec = 1_158_000.0;
        assert_eq!(
            total_cells(METER_DAMAGE, &rows, false),
            ["1.49B", "3.5M", "100%", ""]
        );
        assert_eq!(
            total_cells(METER_DAMAGE, &rows, true),
            ["1.49B", "3.5M", "", ""]
        );
        assert_eq!(sort_of(Col::Avg, Some((Col::Avg, true))), Some(true));
        assert_eq!(sort_of(Col::Hits, Some((Col::Avg, true))), None);
    }
}
