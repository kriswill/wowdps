//! One table primitive for every list of `Row`s the window draws: the
//! column set a row's cells, the heading line over them and the pinned
//! total row under them all share, so a heading sits over its column by
//! construction and the three can never drift. The meter uses a set per
//! view, the inspector's ability list another (the design study's
//! throughput table: amount, share, hits, average hit, crit), its target
//! list a third. Message-generic: the heading's sort message is the
//! caller's.
//!
//! Window-only: its numbers are the window's tabular Barlow (`theme::UI`,
//! the amount in its medium weight) and its heads the tokens' gold-dim
//! labels. The overlay draws rows of its own.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Color, Element, Length, Theme};

use wowdps_model::{Row, View};

pub(crate) use wowdps_gui_logic::table::{Col, counted, figure, overheal_pct, sorted};

use crate::line_icons::{LineIcon, line_icon};
use crate::theme::{self, pitch, size};

/// Gap between columns, and between the headings over them.
pub(crate) const GAP: f32 = 10.0;
/// The live meter's gap (`.thead, .trow, .ttotal{column-gap:12px}`).
pub(crate) const METER_GAP: f32 = 12.0;
/// The heading line's own inset each side: a list whose rows stand `side`
/// from its edges pads its heading line `side` less this.
pub(crate) const HEADS_INSET: f32 = 8.0;
/// The inspector's heading line (`.ihrow{font-size:13px}`).
const INSPECTOR_HEAD_PX: f32 = 13.0;

/// The widths and the gap a table's columns are drawn at: the live meter's
/// — and the enemy drill's attackers, which wear its row — the prototype's
/// own grid for its view (`.v-num4`, `.v-enemy`, `.v-count`), narrowed as
/// it narrows under 820 px; the inspector's lists theirs (`.t-ab`, `.t-tg`,
/// `.cmp2`) at [`GAP`], a column they do not set keeping [`Col::width`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grid {
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
    pub(crate) fn width(self, c: Col) -> f32 {
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
    fn inspector(self) -> bool {
        matches!(self, Grid::Abilities { .. } | Grid::Targets | Grid::Pair)
    }

    /// The heading line's size on this grid: the meter's `.thead` at
    /// 13.5 px, the inspector's `.ihrow{font-size:13px}` — one size for
    /// the whole line, the lead's included.
    pub(crate) fn head_px(self) -> f32 {
        if self.inspector() {
            INSPECTOR_HEAD_PX
        } else {
            size::LABEL
        }
    }

    /// The gap between this grid's columns.
    pub(crate) fn gap(self) -> f32 {
        match self {
            Grid::Abilities { .. } | Grid::Targets | Grid::Pair => GAP,
            Grid::Meter { .. } => METER_GAP,
        }
    }

    /// Width the numeric block claims on this grid: every column plus the
    /// gaps between them.
    pub(crate) fn span(self, cols: &[Col], scale: f32) -> f32 {
        let widths: f32 = cols.iter().map(|&c| self.width(c)).sum();
        (widths + (cols.len().saturating_sub(1)) as f32 * self.gap()) * scale
    }
}

/// The meter in a narrow window: the prototype's two, the amount and the
/// rate (`.v-num4 .c5, .c6 {display:none}` at 820 px).
pub(crate) const METER_NARROW: &[Col] = &[Col::Amount, Col::Rate];

/// The live meter's four figures on Damage and Enemies (`.v-num4`,
/// `.v-enemy`): amount, rate, share, crit.
pub(crate) const METER_DAMAGE: &[Col] = &[Col::Amount, Col::Rate, Col::Pct, Col::CritFine];
/// Healing's: the fourth is the overheal share.
pub(crate) const METER_HEALING: &[Col] = &[Col::Amount, Col::Rate, Col::Pct, Col::Overheal];
/// Taken's (R17): the fourth is what absorbs took.
pub(crate) const METER_TAKEN: &[Col] = &[Col::Amount, Col::Rate, Col::Pct, Col::Absorbed];
/// A count view's two (`.v-count`), at every width: the count and its
/// share — a count has no rate, and blank columns are width a name needs.
pub(crate) const METER_COUNT: &[Col] = &[Col::Amount, Col::Pct];

/// The live meter's columns for `view` (the prototype's `VIEWS.heads`):
/// the amount, the rate, the share and the one figure the view is read
/// by after them — crit, the overheal share, what absorbs took. The
/// overkill no longer rides the meter; a drill still shows it. A narrow
/// window keeps the amount and the rate; a count view its count and share.
pub(crate) fn meter_set(view: View, narrow: bool) -> &'static [Col] {
    match view {
        v if counted(v) => METER_COUNT,
        _ if narrow => METER_NARROW,
        View::Healing => METER_HEALING,
        View::Taken => METER_TAKEN,
        _ => METER_DAMAGE,
    }
}

/// How a [`Col`] is drawn in this GUI: its width and its ink. What the
/// column means — its heading, its cell's text, its sort key — is
/// gui-logic's.
pub(crate) trait ColDraw {
    /// Each column's width at the prototype's 14.5 px figures: its widest
    /// cell ("100.0%", "12345") with a step of air.
    fn width(self) -> f32;
    /// The amount is the number every eye goes to; the rate is second;
    /// everything else — the share, the counts, crit, the overheal and the
    /// absorbed — is secondary (`.num.dim`). The prototype draws crit a
    /// step fainter still (`.num.faint`, INK_3), but INK_3 is under AA on a
    /// panel and under 3.5:1 on the selected row, and these are figures a
    /// reader reads: they stay in INK_2, which clears AA on every row fill.
    fn rank(self) -> u8;
    /// The cell's ink by its rank — or, on the pinned total, parchment for
    /// every figure (`.ttotal .num`).
    fn ink(self, total: bool) -> Color;
}

impl ColDraw for Col {
    fn width(self) -> f32 {
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

    fn rank(self) -> u8 {
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

    fn ink(self, total: bool) -> Color {
        match (total, self.rank()) {
            (true, _) | (false, 0 | 1) => theme::INK,
            _ => theme::INK_2,
        }
    }
}

/// Every column, for the tests that hold a rule over all of them.
#[cfg(test)]
pub(crate) const ALL_COLS: [Col; 9] = [
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

/// A row's numeric cells, in the column set's order, at its span on
/// `grid`: every figure at the prototype's 14.5 px, a row's amount at
/// weight 500, each in its column's ink ([`Col::ink`]) — `total` is the
/// pinned total row's.
pub(crate) fn cells<M: 'static>(
    cols: &[Col],
    grid: Grid,
    r: &Row,
    scale: f32,
    total: bool,
) -> Element<'static, M> {
    let words: Vec<String> = cols.iter().map(|c| c.cell(r)).collect();
    cells_of(cols, grid, words, scale, total)
}

/// A cell's face: a meter row's amount at 500 (`.trow .num.b`), every
/// other figure — the total's amount included (`.ttotal .num`), and every
/// figure in the inspector's lists (`.irow .num`) — at 400.
fn face(c: Col, total: bool, grid: Grid) -> iced::Font {
    if c.rank() == 0 && !total && !grid.inspector() {
        theme::UI_MEDIUM
    } else {
        theme::UI
    }
}

/// [`cells`] with each column's words given.
fn cells_of<M: 'static>(
    cols: &[Col],
    grid: Grid,
    words: Vec<String>,
    scale: f32,
    total: bool,
) -> Element<'static, M> {
    let mut line = row![].spacing(grid.gap() * scale);
    for (&c, words) in cols.iter().zip(words) {
        line = line.push(
            text(words)
                .size(size::NUM * scale)
                .color(c.ink(total))
                .font(face(c, total, grid))
                .wrapping(text::Wrapping::None)
                .width(Length::Fixed(grid.width(c) * scale))
                .align_x(iced::Alignment::End),
        );
    }
    line.width(Length::Fixed(grid.span(cols, scale)))
        .align_y(iced::Alignment::Center)
        .into()
}

/// The air between a sorted heading and its mark (`.thead
/// .h[aria-sort]::after{content:" ↓"}`), whose glyph box is the label's own
/// size ([`Grid::head_px`]): the window's faces carry no arrows, so the
/// mark is a line icon rather than a glyph from whatever system face would
/// stand in.
const ARROW_GAP: f32 = 2.0;

/// How `c` is sorted under `sort`: `Some(true)` descending, `Some(false)`
/// ascending, `None` not at all.
pub(crate) fn sort_of(c: Col, sort: Option<(Col, bool)>) -> Option<bool> {
    sort.and_then(|(s, desc)| (s == c).then_some(desc))
}

/// The heading line: `lead` (the caller's "Player" / "by spell" text with
/// whatever rank column it draws) over the bar's track, then one heading
/// per column on `grid`. With `on_sort` every non-blank heading is a click
/// target that lights gold under the pointer (`.thead .h:hover`); the
/// sorted column is gold, its arrow after the label at the column's end —
/// the label a step left, into the gap before it, as the prototype's
/// ` ↓` pushes it.
pub(crate) fn heads<'a, M: Clone + 'static>(
    cols: &[Col],
    grid: Grid,
    view: View,
    sort: Option<(Col, bool)>,
    on_sort: Option<fn(Col) -> M>,
    lead: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let gap = grid.gap();
    let mut line = row![];
    for &c in cols {
        let head = c.head(view);
        let sorted = sort_of(c, sort);
        // A sorted heading takes the gap before it into its cell, so a long
        // label and its arrow ("Absorbed ↓") have the room without moving
        // a column; every other heading keeps the gap as air.
        let width = grid.width(c) + if sorted.is_some() { gap } else { 0.0 };
        if sorted.is_none() {
            line = line.push(Space::new().width(Length::Fixed(gap)));
        }
        let clickable = on_sort.is_some() && !head.is_empty();
        let label = text(head)
            .size(grid.head_px())
            .font(theme::UI)
            .wrapping(text::Wrapping::None);
        // A click target takes its ink from the button's style, which is
        // what lets the pointer light it; an inert heading is gold-dim.
        let label = if clickable {
            label
        } else {
            label.color(theme::GOLD_DIM)
        };
        let mut words = row![label]
            .spacing(ARROW_GAP)
            .align_y(iced::Alignment::Center);
        if let Some(desc) = sorted {
            let arrow = if desc {
                LineIcon::ArrowDown
            } else {
                LineIcon::ArrowUp
            };
            words = words.push(line_icon(arrow, grid.head_px(), theme::GOLD));
        }
        let cell = container(words)
            .width(Length::Fixed(width))
            .align_x(iced::Alignment::End);
        line = line.push(match on_sort {
            Some(f) if clickable => Element::from(button(cell).padding(0).on_press(f(c)).style(
                move |_: &Theme, status| button::Style {
                    text_color: if sorted.is_some()
                        || matches!(status, button::Status::Hovered | button::Status::Pressed)
                    {
                        theme::GOLD
                    } else {
                        theme::GOLD_DIM
                    },
                    ..button::Style::default()
                },
            )),
            _ => cell.into(),
        });
    }
    let heads = line.width(Length::Fixed(grid.span(cols, 1.0) + gap));
    row![container(lead).width(Length::Fill), heads]
        .padding([0.0, HEADS_INSET])
        .into()
}

/// The total row's inset each side (`.ttotal{padding:0 8px}`) — what a
/// caller lining its label up with a column above subtracts.
pub(crate) const TOTAL_INSET: f32 = 8.0;

/// The pinned total row: the fold of `rows` in the same columns, so a
/// per-row number always has its denominator on screen: the amount and the
/// rate, the share "100%" — blank while a filter narrows the rows
/// (`filtered`), whose share of the chart the total is not — and the
/// view's fourth figure (crit, overheal, absorbed) left blank, a rate of a
/// sum being no figure a reader reads there. A surface under a hairline
/// (`.ttotal{background:var(--surface);border-top:1px solid var(--line)}`),
/// the row's full width; the label is the frame's 14 px (`.ttotal` sets no
/// size of its own).
pub(crate) fn total<M: 'static>(
    cols: &[Col],
    grid: Grid,
    rows: &[Row],
    label: String,
    lead_pad: f32,
    filtered: bool,
) -> Element<'static, M> {
    let sum = Row {
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
        mine: false,
        offset_ms: None,
    };
    let words: Vec<String> = cols
        .iter()
        .map(|c| match c {
            Col::Pct if filtered => String::new(),
            Col::Pct => "100%".to_string(),
            Col::Crit | Col::CritFine | Col::Overheal | Col::Absorbed => String::new(),
            _ => c.cell(&sum),
        })
        .collect();
    // The label is ONE line, clipped: a narrow pane would otherwise wrap
    // "players" under the row's box, onto whatever follows it.
    let lead = row![
        Space::new().width(Length::Fixed(lead_pad)),
        container(
            text(label)
                .size(size::FRAME)
                .color(theme::INK_2)
                .wrapping(text::Wrapping::None)
        )
        .clip(true)
        .width(Length::Fill),
    ]
    .spacing(GAP);
    let line = container(
        row![
            container(lead).width(Length::Fill),
            cells_of::<M>(cols, grid, words, 1.0, true)
        ]
        .spacing(grid.gap())
        .padding(iced::Padding {
            top: 0.0,
            // The live meter's total runs the stage's width, under the
            // list's scrollbar lane too; its figures stay over the rows'.
            right: TOTAL_INSET + pitch::SCROLL_LANE,
            bottom: 0.0,
            left: TOTAL_INSET,
        })
        .height(Length::Fill)
        .align_y(iced::Alignment::Center),
    )
    .height(pitch::TOTAL - 1.0)
    .width(Length::Fill)
    .style(|_: &Theme| container::Style {
        background: Some(theme::SURFACE.into()),
        ..container::Style::default()
    });
    column![crate::nav::hairline(), line]
        .width(Length::Fill)
        .into()
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
            mine: false,
            offset_ms: None,
        }
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
    /// a comparison's `.cmp2` (50 40), all 10 px apart.
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
        }
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

    /// A meter row's amount is set at 500; the total's figures, its amount
    /// included, are the prototype's plain `.num` at 400 — and so is every
    /// figure in the inspector's lists (`.irow .num`), the amount's too.
    /// The inspector's heading line is one 13 px size, the meter's 13.5.
    #[test]
    fn only_a_meter_row_s_amount_is_set_heavier() {
        let meter = Grid::Meter {
            view: View::Damage,
            narrow: false,
        };
        assert_eq!(face(Col::Amount, false, meter), theme::UI_MEDIUM);
        assert_eq!(face(Col::Amount, true, meter), theme::UI);
        for c in ALL_COLS.into_iter().filter(|c| *c != Col::Amount) {
            assert_eq!(face(c, false, meter), theme::UI, "{c:?}");
            assert_eq!(face(c, true, meter), theme::UI, "{c:?}");
        }
        for grid in [Grid::Abilities { narrow: false }, Grid::Targets, Grid::Pair] {
            assert_eq!(face(Col::Amount, false, grid), theme::UI, "{grid:?}");
            assert_eq!(grid.head_px(), 13.0, "{grid:?}");
        }
        assert_eq!(meter.head_px(), size::LABEL);
    }

    /// The figures step down as the prototype's do: amount and rate in
    /// ink, everything else secondary — crit included, which the prototype
    /// draws fainter but a reader must read; the pinned total is all ink.
    #[test]
    fn the_cells_ink_by_their_column() {
        assert_eq!(Col::Amount.ink(false), theme::INK);
        assert_eq!(Col::Rate.ink(false), theme::INK);
        assert_eq!(Col::Pct.ink(false), theme::INK_2);
        assert_eq!(Col::Hits.ink(false), theme::INK_2);
        assert_eq!(Col::Crit.ink(false), theme::INK_2);
        for c in ALL_COLS {
            assert_eq!(c.ink(true), theme::INK, "{c:?}");
        }
    }

    #[test]
    fn the_heading_line_marks_the_sort_and_only_named_columns_click() {
        #[derive(Debug, Clone, PartialEq)]
        enum M {
            Sort(Col),
        }
        let mut ui = simulator(heads::<M>(
            &[Col::Amount, Col::Pct, Col::Hits, Col::Crit, Col::Avg],
            Grid::Abilities { narrow: false },
            View::Damage,
            Some((Col::Avg, true)),
            Some(M::Sort),
            text("by spell"),
        ));
        // The arrow is a line icon after the label, not a glyph the
        // window's faces lack: the label reads as it always does.
        assert!(ui.find("Avg").is_ok());
        assert_eq!(sort_of(Col::Avg, Some((Col::Avg, true))), Some(true));
        assert_eq!(sort_of(Col::Avg, Some((Col::Avg, false))), Some(false));
        assert_eq!(sort_of(Col::Hits, Some((Col::Avg, true))), None);
        let _ = ui.snapshot(&Theme::TokyoNight).unwrap();
        assert!(ui.find("Hits").is_ok());
        ui.click("Hits").unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![M::Sort(Col::Hits)]
        );
        // A count view names no rate, so its rate heading is inert.
        let mut ui = simulator(heads::<M>(
            &[Col::Amount, Col::Rate, Col::Pct],
            Grid::Meter {
                view: View::Dispels,
                narrow: false,
            },
            View::Dispels,
            None,
            Some(M::Sort),
            text("player"),
        ));
        assert!(ui.find("Count").is_ok());
        assert!(ui.find("Per sec").is_err());
    }

    /// The live meter's total is the prototype's: the amount to two
    /// places at a billion ("1.49B", beside rows of "92.7M"), the rate, the
    /// share "100%" — blank under a filter — and the fourth figure blank.
    #[test]
    fn the_meter_s_total_says_what_the_prototype_s_does() {
        let mut rows = vec![row("a", 1_000_000_000, 10, 5), row("b", 488_795_375, 10, 1)];
        rows[0].per_sec = 2_369_000.0;
        rows[1].per_sec = 1_158_000.0;
        for (filtered, share) in [(false, true), (true, false)] {
            let mut ui = simulator(total::<()>(
                METER_DAMAGE,
                Grid::Meter {
                    view: View::Damage,
                    narrow: false,
                },
                &rows,
                "Total, 2 players".to_string(),
                0.0,
                filtered,
            ));
            assert!(ui.find("1.49B").is_ok(), "two places at a billion");
            assert!(ui.find("3.5M").is_ok());
            assert_eq!(ui.find("100%").is_ok(), share, "filtered: {filtered}");
            assert!(ui.find("100.0%").is_err());
            assert!(ui.find("30.0%").is_err(), "no crit on the total");
        }
        let _ = render(cells::<()>(
            &[Col::Amount, Col::Pct, Col::Hits],
            Grid::Targets,
            &rows[0],
            1.5,
            false,
        ));
    }
}
