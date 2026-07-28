//! Rendering. Nothing here mutates state.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Pane};
use crate::model::{Row, View};

const METER_HINTS: &str =
    "d dmg  h heal  i intr  c cc  x disp  K deaths | [ ] seg | j/k move | enter drill | q quit";
const DRILL_HINTS: &str = "tab pane | j/k move | esc back | d h i c x K view | q quit";

/// `12.3k`, `1.2M` — meter-style short numbers.
pub fn human(n: u64) -> String {
    const UNITS: [&str; 3] = ["k", "M", "B"];
    if n < 1_000 {
        return n.to_string();
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1_000.0 && unit < UNITS.len() {
        value /= 1_000.0;
        unit += 1;
    }
    // One decimal place can round 999.99k up to "1000.0k"; promote instead.
    if value >= 999.95 && unit < UNITS.len() {
        value /= 1_000.0;
        unit += 1;
    }
    format!("{value:.1}{}", UNITS[unit - 1])
}

/// `2:14`, `1:02:03`.
pub fn duration(ms: i64) -> String {
    let total = (ms.max(0) / 1000) as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn view_name(view: View) -> &'static str {
    match view {
        View::Damage => "Damage",
        View::Healing => "Healing",
        View::Interrupts => "Interrupts",
        View::CrowdControl => "Crowd Control",
        View::Dispels => "Dispels",
        View::Deaths => "Deaths",
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, header, app);
    if app.drill.is_some() {
        draw_drilldown(frame, body, app);
    } else {
        draw_meter(frame, body, app);
    }
    draw_footer(frame, footer, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let left = match app.segment() {
        Some(seg) => {
            let state = if app.is_live() {
                "LIVE"
            } else {
                match seg.success {
                    Some(true) => "Kill",
                    Some(false) => "Wipe",
                    None => "Done",
                }
            };
            let mut s = format!(
                "[{}/{}] {}  {}  {}",
                app.segment_index() + 1,
                app.segment_count(),
                seg.name,
                duration(app.duration_ms()),
                state,
            );
            if !app.following_live() {
                s.push_str(" (history)");
            }
            if let Some(drill) = app.drill.as_ref() {
                s.push_str(&format!("  > {}", drill.label));
            }
            s
        }
        None => "no segments".to_string(),
    };
    let mid = app
        .source
        .clone()
        .unwrap_or_else(|| "waiting for log file".to_string());
    let text = compose(&left, &mid, view_name(app.view), area.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(text)).style(
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hints = if app.drill.is_some() {
        DRILL_HINTS
    } else {
        METER_HINTS
    };
    let width = area.width as usize;
    let text = match app.status.as_ref() {
        Some(err) => {
            let err = format!("! {err} ");
            let room = width.saturating_sub(err.chars().count());
            let mut s = truncate(hints, room);
            s.push_str(&" ".repeat(room.saturating_sub(s.chars().count())));
            s.push_str(&err);
            truncate(&s, width)
        }
        None => truncate(hints, width),
    };
    let style = if app.status.is_some() {
        Style::new().fg(Color::Red)
    } else {
        Style::new().fg(Color::DarkGray)
    };
    frame.render_widget(Paragraph::new(Line::from(text)).style(style), area);
}

fn draw_meter(frame: &mut Frame, area: Rect, app: &App) {
    let rows = app.rows();
    if rows.is_empty() {
        return draw_empty(frame, area, app.view);
    }
    draw_rows(frame, area, &rows, app.row_sel, true, app.view);
}

fn draw_drilldown(frame: &mut Frame, area: Rect, app: &App) {
    let Some(drill) = app.drill.as_ref() else {
        return;
    };
    let (by_spell, by_target) = app.breakdown();
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);

    for (rect, title, rows, sel, focused) in [
        (
            left,
            " By spell ",
            &by_spell,
            drill.spell_sel,
            drill.pane == Pane::Spell,
        ),
        (
            right,
            " By target ",
            &by_target,
            drill.target_sel,
            drill.pane == Pane::Target,
        ),
    ] {
        let block = Block::bordered().title(title).border_style(if focused {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new().fg(Color::DarkGray)
        });
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        if rows.is_empty() {
            draw_empty(frame, inner, app.view);
        } else {
            draw_rows(frame, inner, rows, sel, focused, app.view);
        }
    }
}

fn draw_empty(frame: &mut Frame, area: Rect, view: View) {
    let text = format!(
        "No {} recorded in this segment.",
        view_name(view).to_lowercase()
    );
    frame.render_widget(
        Paragraph::new(Line::from(text))
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::DarkGray)),
        area,
    );
}

fn draw_rows(frame: &mut Frame, area: Rect, rows: &[Row], sel: usize, focused: bool, view: View) {
    let height = area.height as usize;
    if height == 0 || area.width == 0 {
        return;
    }
    // Keep the selection on screen: scroll only once it would fall off the end.
    let offset = sel.saturating_sub(height - 1);
    let max = rows.first().map(|r| r.amount).unwrap_or(0).max(1);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, row)| {
            let selected = i == sel;
            let text = row_text(
                i + 1,
                row,
                max,
                area.width as usize,
                selected && focused,
                view,
            );
            let style = match (selected, focused) {
                (true, true) => Style::new().fg(Color::Black).bg(Color::Cyan),
                (true, false) => Style::new().add_modifier(Modifier::BOLD),
                _ => Style::new(),
            };
            Line::from(text).style(style)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

struct Cols {
    name: usize,
    bar: usize,
    amount: usize,
    rate: usize,
    pct: usize,
}

/// Columns drop right-to-left as the terminal narrows; the bar absorbs slack.
fn columns(width: usize) -> Cols {
    let name = (width / 3).clamp(6, 20).min(width);
    let pct = if width >= 30 { 6 } else { 0 };
    let rate = if width >= 60 { 9 } else { 0 };
    let amount = if width >= 44 { 9 } else { 0 };
    let fixed = 4
        + name
        + 1
        + if amount > 0 { amount + 1 } else { 0 }
        + if rate > 0 { rate + 1 } else { 0 }
        + if pct > 0 { pct + 1 } else { 0 };
    Cols {
        name,
        bar: width.saturating_sub(fixed),
        amount,
        rate,
        pct,
    }
}

fn row_text(rank: usize, row: &Row, max: u64, width: usize, selected: bool, view: View) -> String {
    let c = columns(width);
    let filled = if c.bar == 0 {
        0
    } else {
        (row.amount as u128 * c.bar as u128 / max as u128) as usize
    };
    let bar = "█".repeat(filled.min(c.bar));

    let mut s = format!(
        "{}{rank:>2} {:<name$}",
        if selected { '>' } else { ' ' },
        truncate(&row.label, c.name),
        name = c.name,
    );
    if c.bar > 0 {
        s.push_str(&format!(" {bar:<w$}", w = c.bar));
    }
    if c.amount > 0 {
        s.push_str(&format!(" {:>w$}", human(row.amount), w = c.amount));
    }
    if c.rate > 0 {
        let rate = if is_rate_view(view) {
            human(row.per_sec as u64)
        } else {
            "-".to_string()
        };
        s.push_str(&format!(" {rate:>w$}", w = c.rate));
    }
    if c.pct > 0 {
        s.push_str(&format!(" {:>w$}", format!("{:.1}%", row.pct), w = c.pct));
    }
    truncate(&s, width)
}

fn is_rate_view(view: View) -> bool {
    matches!(view, View::Damage | View::Healing)
}

/// `left`, then `mid`, then `right` hard against the right edge — dropping the
/// pieces that don't fit rather than overflowing.
fn compose(left: &str, mid: &str, right: &str, width: usize) -> String {
    let (l, m, r) = (
        left.chars().count(),
        mid.chars().count(),
        right.chars().count(),
    );
    let mut s = left.to_string();
    if width >= l + 2 + m + 2 + r {
        s.push_str("  ");
        s.push_str(mid);
        s.push_str(&" ".repeat(width - s.chars().count() - r));
        s.push_str(right);
    } else if width >= l + 2 + r {
        s.push_str(&" ".repeat(width - l - r));
        s.push_str(right);
    }
    truncate(&s, width)
}

fn truncate(s: &str, width: usize) -> String {
    s.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// The rendered buffer as one string per row.
    fn render(app: &App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    fn flat(lines: &[String]) -> String {
        lines.join("\n")
    }

    fn row_index(lines: &[String], needle: &str) -> usize {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} not rendered in:\n{}", flat(lines)))
    }

    #[test]
    fn human_numbers_read_like_a_damage_meter() {
        assert_eq!(human(0), "0");
        assert_eq!(human(999), "999");
        assert_eq!(human(1_000), "1.0k");
        assert_eq!(human(12_345), "12.3k");
        assert_eq!(human(1_234_567), "1.2M");
        assert_eq!(human(2_500_000_000), "2.5B");
    }

    #[test]
    fn human_promotes_instead_of_rendering_1000_of_a_unit() {
        assert_eq!(human(999_999), "1.0M");
        assert_eq!(human(999_999_999), "1.0B");
    }

    #[test]
    fn durations_are_mm_ss_until_an_hour() {
        assert_eq!(duration(0), "0:00");
        assert_eq!(duration(9_000), "0:09");
        assert_eq!(duration(134_000), "2:14");
        assert_eq!(duration(3_599_000), "59:59");
        assert_eq!(duration(3_723_000), "1:02:03");
        assert_eq!(duration(-5), "0:00", "never render a negative clock");
    }

    #[test]
    fn every_view_has_a_name() {
        for (view, name) in [
            (View::Damage, "Damage"),
            (View::Healing, "Healing"),
            (View::Interrupts, "Interrupts"),
            (View::CrowdControl, "Crowd Control"),
            (View::Dispels, "Dispels"),
            (View::Deaths, "Deaths"),
        ] {
            assert_eq!(view_name(view), name);
        }
    }

    #[test]
    fn meter_view_shows_encounter_view_and_rows_in_order() {
        let app = App::new();
        let lines = render(&app, 100, 20);
        let all = flat(&lines);

        let seg = app.segment().unwrap();
        assert!(
            all.contains(&seg.name),
            "header names the encounter:\n{all}"
        );
        assert!(all.contains("Damage"), "header names the view:\n{all}");
        assert!(all.contains("LIVE"), "live segment is marked:\n{all}");

        let rows = app.rows();
        assert_eq!(rows.len(), 3);
        let positions: Vec<usize> = rows.iter().map(|r| row_index(&lines, &r.label)).collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "rows must render in meter order (desc by amount):\n{all}"
        );
    }

    #[test]
    fn meter_rows_carry_amount_rate_and_pct() {
        let app = App::new();
        let lines = render(&app, 100, 20);
        let top = &app.rows()[0];
        let line = &lines[row_index(&lines, &top.label)];

        assert!(line.contains(&human(top.amount)), "amount missing: {line}");
        assert!(
            line.contains(&human(top.per_sec as u64)),
            "per-second missing: {line}"
        );
        assert!(
            line.contains(&format!("{:.1}%", top.pct)),
            "pct missing: {line}"
        );
        assert!(line.contains('█'), "bar missing: {line}");
    }

    #[test]
    fn the_selected_row_is_marked() {
        let mut app = App::new();
        app.apply(Action::Down);
        let lines = render(&app, 100, 20);
        let selected = app.rows()[1].label.clone();
        let line = &lines[row_index(&lines, &selected)];
        assert!(
            line.trim_start().starts_with('>'),
            "selection marker missing: {line:?}"
        );
    }

    #[test]
    fn count_views_render_a_dash_instead_of_a_rate() {
        let mut app = App::new();
        app.apply(Action::SetView(View::Interrupts));
        let rows = app.rows();
        assert!(!rows.is_empty(), "expected interrupt rows in the demo data");
        let lines = render(&app, 100, 20);
        let line = &lines[row_index(&lines, &rows[0].label)];
        assert!(
            line.contains(" - "),
            "expected a dash for per_sec: {line:?}"
        );
    }

    #[test]
    fn drilldown_shows_both_panes_for_the_selected_player() {
        let mut app = App::new();
        app.apply(Action::Open);
        let drill = app.drill.clone().unwrap();
        let (by_spell, by_target) = app.breakdown();

        let lines = render(&app, 120, 20);
        let all = flat(&lines);
        assert!(
            all.contains(&drill.label),
            "drilldown names the player:\n{all}"
        );
        assert!(all.contains("By spell"), "spell pane title missing:\n{all}");
        assert!(
            all.contains("By target"),
            "target pane title missing:\n{all}"
        );
        assert!(
            all.contains(&by_spell[0].label),
            "top spell missing:\n{all}"
        );
        assert!(
            all.contains(&by_target[0].label),
            "top target missing:\n{all}"
        );
    }

    #[test]
    fn drilldown_marks_the_focused_pane_selection_only() {
        let mut app = App::new();
        app.apply(Action::Open);
        app.apply(Action::Down);
        let (by_spell, _) = app.breakdown();
        let lines = render(&app, 120, 20);
        let line = &lines[row_index(&lines, &by_spell[1].label)];
        assert!(
            line.contains('>'),
            "focused pane selection missing: {line:?}"
        );
    }

    #[test]
    fn a_finished_segment_shows_its_result_not_live() {
        let mut app = App::new();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        let all = flat(&render(&app, 100, 20));
        assert!(all.contains("Kel'Thuzad"), "{all}");
        assert!(!all.contains("LIVE"), "closed segment is not live:\n{all}");
        assert!(all.contains("Kill"), "kill/wipe result missing:\n{all}");
    }

    #[test]
    fn an_empty_view_says_so_instead_of_rendering_nothing() {
        let mut app = App::new();
        app.apply(Action::SetView(View::Deaths));
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        assert!(app.rows().is_empty());
        let all = flat(&render(&app, 100, 20));
        assert!(
            all.to_lowercase().contains("no "),
            "expected an empty-state message:\n{all}"
        );
    }

    #[test]
    fn the_footer_documents_the_keybinds() {
        let app = App::new();
        let all = flat(&render(&app, 120, 20));
        for hint in ["d", "h", "i", "c", "x", "K", "[", "]", "q"] {
            assert!(all.contains(hint), "footer missing {hint:?}:\n{all}");
        }
    }

    #[test]
    fn waiting_for_a_log_file_is_shown_in_the_header() {
        let app = App::new();
        let all = flat(&render(&app, 100, 20));
        assert!(all.to_lowercase().contains("waiting"), "{all}");
    }

    #[test]
    fn tail_errors_surface_in_the_footer() {
        let mut app = App::new();
        app.on_tail(crate::tail::TailEvent::Error("permission denied".into()));
        let all = flat(&render(&app, 100, 20));
        assert!(all.contains("permission denied"), "{all}");
    }

    #[test]
    fn tiny_terminals_render_without_panicking() {
        let mut app = App::new();
        for (w, h) in [(1, 1), (4, 2), (20, 3), (39, 10), (59, 8), (200, 60)] {
            render(&app, w, h);
        }
        app.apply(Action::Open);
        for (w, h) in [(1, 1), (4, 2), (20, 3), (39, 10), (200, 60)] {
            render(&app, w, h);
        }
    }

    #[test]
    fn long_row_lists_scroll_to_keep_the_selection_visible() {
        let mut app = App::new();
        app.apply(Action::Open);
        let (by_spell, _) = app.breakdown();
        assert!(by_spell.len() >= 3);
        // A pane only two rows tall must still show the selected last row.
        for _ in 0..by_spell.len() {
            app.apply(Action::Down);
        }
        let last = by_spell.last().unwrap().label.clone();
        let all = flat(&render(&app, 120, 8));
        assert!(
            all.contains(&last),
            "selection scrolled out of view:\n{all}"
        );
    }
}
