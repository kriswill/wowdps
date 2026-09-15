//! One table primitive for every list of `Row`s the window draws: the
//! column set a row's cells, the heading line over them and the pinned
//! total row under them all share, so a heading sits over its column by
//! construction and the three can never drift. The meter uses one set,
//! the by-spell drill pane another (the design study's throughput table:
//! amount, share, hits, average hit, crit, rate), the by-target pane a
//! third. Message-generic: the heading's sort message is the caller's.

use iced::widget::{Space, container, mouse_area, row, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::human;
use wowdps_model::{Row, View};

use crate::theme::{self, DIM, size};

/// A numeric column. Every one is derivable from a `Row` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Col {
    /// Overkill / overheal / absorbed, in parentheses.
    Extra,
    Amount,
    /// Per second.
    Rate,
    /// Share of the list's total.
    Pct,
    /// Crit rate over the contributing events.
    Crit,
    /// Contributing events: hits/ticks, heals, or the recorded count.
    Hits,
    /// Average per event.
    Avg,
}

/// Gap between columns, and between the headings over them.
pub(crate) const GAP: f32 = 10.0;

/// The meter's columns (extra first, so the parenthesised overkill sits
/// away from the bar's edge, then the amount every eye goes to).
pub(crate) const METER: &[Col] = &[Col::Extra, Col::Amount, Col::Rate, Col::Pct, Col::Crit];
/// The by-spell drill pane: Archon's damage-breakdown columns.
pub(crate) const SPELLS: &[Col] = &[
    Col::Amount,
    Col::Pct,
    Col::Hits,
    Col::Avg,
    Col::Crit,
    Col::Rate,
];
/// The by-target pane, half a window wide beside the spells.
pub(crate) const TARGETS: &[Col] = &[Col::Amount, Col::Pct, Col::Hits];

fn counted(view: View) -> bool {
    matches!(
        view,
        View::Interrupts | View::CrowdControl | View::Dispels | View::Deaths
    )
}

impl Col {
    pub(crate) fn width(self) -> f32 {
        match self {
            Col::Extra => 64.0,
            Col::Amount => 56.0,
            Col::Rate => 52.0,
            Col::Pct => 44.0,
            Col::Crit => 40.0,
            Col::Hits => 44.0,
            Col::Avg => 52.0,
        }
    }

    /// The heading in a view. `""` means the column is meaningless there:
    /// it is drawn blank and takes no click.
    pub(crate) fn head(self, view: View) -> &'static str {
        match self {
            Col::Extra => match view {
                View::Damage => "(overkill)",
                View::Healing => "(overheal)",
                View::Taken | View::EnemyTaken => "(absorbed)",
                _ => "",
            },
            Col::Amount => match view {
                View::Taken | View::EnemyTaken => "taken",
                v if counted(v) => "count",
                _ => "total",
            },
            Col::Rate => match view {
                View::Healing => "hps",
                View::Taken | View::EnemyTaken => "dtps",
                View::Damage => "dps",
                _ => "",
            },
            Col::Pct => "%",
            Col::Crit => {
                if counted(view) {
                    ""
                } else {
                    "crit"
                }
            }
            Col::Hits => {
                if counted(view) {
                    ""
                } else {
                    "hits"
                }
            }
            Col::Avg => {
                if counted(view) {
                    ""
                } else {
                    "avg"
                }
            }
        }
    }

    /// The cell's text; blank where the row has nothing to say (no
    /// overkill, no crits, no events) rather than a misleading 0.
    pub(crate) fn cell(self, r: &Row) -> String {
        match self {
            Col::Extra if r.extra > 0 => format!("({})", human(r.extra)),
            Col::Amount => human(r.amount),
            Col::Rate if r.per_sec >= 1.0 => human(r.per_sec as u64),
            Col::Pct => format!("{:>4.1}%", r.pct),
            Col::Crit if r.crits > 0 => format!("{:.0}%", r.crit_pct()),
            Col::Hits if r.count > 0 => r.count.to_string(),
            Col::Avg if r.count > 0 => human(r.amount / r.count),
            _ => String::new(),
        }
    }

    /// The value a sort on this column orders by.
    pub(crate) fn key(self, r: &Row) -> f64 {
        match self {
            Col::Extra => r.extra as f64,
            Col::Amount => r.amount as f64,
            Col::Rate => r.per_sec,
            Col::Pct => r.pct,
            Col::Crit => r.crit_pct(),
            Col::Hits => r.count as f64,
            Col::Avg => {
                if r.count > 0 {
                    r.amount as f64 / r.count as f64
                } else {
                    0.0
                }
            }
        }
    }

    /// The amount is the number every eye goes to; the rate is second;
    /// the rest are tertiary.
    fn rank(self) -> u8 {
        match self {
            Col::Amount => 0,
            Col::Rate => 1,
            _ => 2,
        }
    }
}

/// Width the numeric block claims: every column plus the gaps between
/// them. The bar's track is whatever is left of the row, which is what
/// keeps the fill out from under the numbers at every bar length.
pub(crate) fn span(cols: &[Col], scale: f32) -> f32 {
    let widths: f32 = cols.iter().map(|c| c.width()).sum();
    (widths + (cols.len().saturating_sub(1)) as f32 * GAP) * scale
}

/// A row's numeric cells, in the column set's order, at its span. The ink
/// trio is (primary, secondary, tertiary), chosen by the caller for what
/// sits under the numbers.
pub(crate) fn cells<M: 'static>(
    cols: &[Col],
    r: &Row,
    scale: f32,
    ink: (Color, Color, Color),
) -> Element<'static, M> {
    let mut line = row![].spacing(GAP * scale);
    for c in cols {
        let (size, color) = match c.rank() {
            0 => (13.0, ink.0),
            1 => (12.0, ink.1),
            _ => (11.0, ink.2),
        };
        let align = iced::Alignment::End;
        line = line.push(
            text(c.cell(r))
                .size(size * scale)
                .color(color)
                .font(Font::MONOSPACE)
                .width(Length::Fixed(c.width() * scale))
                .align_x(align),
        );
    }
    line.width(Length::Fixed(span(cols, scale)))
        .align_y(iced::Alignment::Center)
        .into()
}

/// The heading line: `lead` (the caller's "player" / "by spell" text with
/// whatever rank column it draws) over the bar's track, then one heading
/// per column. With `on_sort` every non-blank heading is a click target;
/// the sorted column is marked ▾ / ▴ and lit.
pub(crate) fn heads<'a, M: Clone + 'static>(
    cols: &[Col],
    view: View,
    sort: Option<(Col, bool)>,
    on_sort: Option<fn(Col) -> M>,
    lead: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let mut line = row![].spacing(GAP);
    for &c in cols {
        let head = c.head(view);
        let marker = match sort {
            Some((s, true)) if s == c => " ▾",
            Some((s, false)) if s == c => " ▴",
            _ => "",
        };
        let label = text(format!("{head}{marker}"))
            .size(size::TINY)
            .color(if marker.is_empty() { DIM } else { Color::WHITE })
            .font(Font::MONOSPACE)
            .width(Length::Fixed(c.width()))
            .align_x(iced::Alignment::End);
        line = line.push(match on_sort {
            Some(f) if !head.is_empty() => Element::from(mouse_area(label).on_press(f(c))),
            _ => label.into(),
        });
    }
    let heads = line.width(Length::Fixed(span(cols, 1.0)));
    row![container(lead).width(Length::Fill), heads]
        .spacing(GAP)
        .padding([0, 8])
        .into()
}

/// The pinned total row: the fold of `rows` in the same columns, so a
/// per-row number always has its denominator on screen. Share reads 100%
/// and the average is the fold's own.
pub(crate) fn total<M: 'static>(
    cols: &[Col],
    rows: &[Row],
    label: String,
    lead_pad: f32,
) -> Element<'static, M> {
    let fold = Row {
        key: String::new(),
        label: String::new(),
        amount: rows.iter().map(|r| r.amount).sum(),
        extra: rows.iter().map(|r| r.extra).sum(),
        count: rows.iter().map(|r| r.count).sum(),
        crits: rows.iter().map(|r| r.crits).sum(),
        per_sec: rows.iter().map(|r| r.per_sec).sum(),
        pct: 100.0,
        class: None,
        spec: None,
        hp: None,
        gain: false,
        spell_id: 0,
        enemy: false,
        school: 0,
    };
    let ink = (Color::WHITE, Color::from_rgba(1.0, 1.0, 1.0, 0.75), DIM);
    let lead = row![
        Space::new().width(Length::Fixed(lead_pad)),
        text(label).size(size::SMALL).color(DIM).width(Length::Fill),
    ]
    .spacing(GAP);
    container(
        row![
            container(lead).width(Length::Fill),
            cells::<M>(cols, &fold, 1.0, ink)
        ]
        .spacing(GAP)
        .padding([0, 8])
        .align_y(iced::Alignment::Center),
    )
    .height(24)
    .width(Length::Fill)
    .style(|_: &Theme| container::Style {
        border: Border {
            color: theme::RULE,
            width: 1.0,
            radius: 0.into(),
        },
        ..container::Style::default()
    })
    .into()
}

/// `rows` in the drawn order: stable-sorted by `sort` when there is one,
/// each with the index it arrived with — the index a click sends back and
/// the rank a row keeps. `None` is the order the rows came in.
pub(crate) fn sorted(mut rows: Vec<(usize, Row)>, sort: Option<(Col, bool)>) -> Vec<(usize, Row)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{render, simulator};

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
        }
    }

    #[test]
    fn cells_stay_blank_where_a_row_has_nothing_to_say() {
        let r = row("Melee", 0, 0, 0);
        assert_eq!(Col::Extra.cell(&r), "");
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
        assert_eq!(Col::Rate.head(View::Damage), "dps");
        assert_eq!(Col::Rate.head(View::Healing), "hps");
        assert_eq!(Col::Rate.head(View::Taken), "dtps");
        assert_eq!(Col::Rate.head(View::Interrupts), "");
        assert_eq!(Col::Amount.head(View::Dispels), "count");
        assert_eq!(Col::Amount.head(View::Taken), "taken");
        assert_eq!(Col::Extra.head(View::Healing), "(overheal)");
        assert_eq!(Col::Crit.head(View::Deaths), "");
    }

    #[test]
    fn the_span_is_the_columns_plus_the_gaps_between_them() {
        assert_eq!(span(&[Col::Amount], 1.0), 56.0);
        assert_eq!(span(&[Col::Amount, Col::Pct], 1.0), 56.0 + 44.0 + GAP);
        assert_eq!(
            span(&[Col::Amount, Col::Pct], 2.0),
            2.0 * (56.0 + 44.0 + GAP)
        );
        assert_eq!(span(&[], 1.0), 0.0);
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
    fn the_heading_line_marks_the_sort_and_only_named_columns_click() {
        #[derive(Debug, Clone, PartialEq)]
        enum M {
            Sort(Col),
        }
        let mut ui = simulator(heads::<M>(
            SPELLS,
            View::Damage,
            Some((Col::Avg, true)),
            Some(M::Sort),
            text("by spell"),
        ));
        assert!(ui.find("avg ▾").is_ok());
        assert!(ui.find("hits").is_ok());
        ui.click("hits").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![M::Sort(Col::Hits)]
        );
        // A count view names no rate, so its rate heading is inert.
        let mut ui = simulator(heads::<M>(
            METER,
            View::Dispels,
            None,
            Some(M::Sort),
            text("player"),
        ));
        assert!(ui.find("count").is_ok());
        assert!(ui.find("dps").is_err());
    }

    #[test]
    fn the_total_row_folds_the_rows() {
        let rows = vec![row("a", 100, 2, 1), row("b", 300, 2, 1)];
        let mut ui = simulator(total::<()>(SPELLS, &rows, "total · 2".to_string(), 14.0));
        assert!(ui.find("total · 2").is_ok());
        assert!(ui.find("400").is_ok(), "Σ amount");
        assert!(ui.find("100").is_ok(), "the fold's own average, 400 / 4");
        assert!(ui.find("50%").is_ok(), "Σ crits / Σ count");
        assert!(
            ui.find("100.0%").is_ok(),
            "the share of everything is everything"
        );
        let _ = render(cells::<()>(
            METER,
            &rows[0],
            1.5,
            (Color::WHITE, Color::WHITE, DIM),
        ));
    }
}
