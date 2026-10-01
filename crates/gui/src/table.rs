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
//! labels. The overlay draws rows of its own. What the columns mean and how
//! wide they stand is gui-logic's (`table`).

use iced::widget::{Space, button, column, container, row, text};
use iced::{Color, Element, Length, Theme};

use wowdps_model::{Row, View};

#[cfg(test)]
pub(crate) use wowdps_gui_logic::table::{ALL_COLS, METER_DAMAGE};
pub(crate) use wowdps_gui_logic::table::{
    Col, GAP, Grid, HEADS_INSET, METER_NARROW, TOTAL_INSET, figure, meter_set, overheal_pct,
    sort_of, sorted, total_cells,
};

use crate::line_icons::{LineIcon, line_icon};
use crate::theme::{self, pitch, size};

/// How a [`Col`] is inked in this GUI. What the column means — its heading,
/// its cell's text, its sort key, its width and its rank — is gui-logic's.
pub(crate) trait ColDraw {
    /// The cell's ink by its rank — or, on the pinned total, parchment for
    /// every figure (`.ttotal .num`). The prototype draws crit a step
    /// fainter still (`.num.faint`, INK_3), but INK_3 is under AA on a
    /// panel and under 3.5:1 on the selected row, and these are figures a
    /// reader reads: they stay in INK_2, which clears AA on every row fill.
    fn ink(self, total: bool) -> Color;
}

impl ColDraw for Col {
    fn ink(self, total: bool) -> Color {
        match (total, self.rank()) {
            (true, _) | (false, 0 | 1) => theme::INK,
            _ => theme::INK_2,
        }
    }
}

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

/// The pinned total row: the fold of `rows` in the same columns
/// ([`total_cells`]), so a per-row number always has its denominator on
/// screen. A surface under a hairline (`.ttotal{background:var(--surface);
/// border-top:1px solid var(--line)}`), the row's full width; the label is
/// the frame's 14 px (`.ttotal` sets no size of its own).
pub(crate) fn total<M: 'static>(
    cols: &[Col],
    grid: Grid,
    rows: &[Row],
    label: String,
    lead_pad: f32,
    filtered: bool,
) -> Element<'static, M> {
    let words = total_cells(cols, rows, filtered);
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
