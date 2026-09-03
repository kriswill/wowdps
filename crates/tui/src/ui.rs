//! Rendering. Nothing here mutates state.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use wowdps_model::fmt;
use wowdps_model::{ListRow, Pane, Screen};
use wowdps_model::{Row, SegmentKind, View};
use wowdps_proto::ClientState;

const METER_HINTS: &str = "d dmg  h heal  i intr  c cc  x disp  K deaths | [ ] seg | j/k move | enter drill | esc list | q quit";
const DRILL_HINTS: &str = "tab pane | j/k move | esc back | d h i c x K view | q quit";
const LIST_HINTS: &str = "j/k move | enter open | q quit";

pub use wowdps_model::fmt::{duration, human, view_name};

pub fn draw(frame: &mut Frame, app: &ClientState) {
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
    match app.screen {
        Screen::List => draw_list(frame, body, app),
        Screen::Meter if app.drill.is_some() => draw_drilldown(frame, body, app),
        // R12 is a GUI feature: the TUI keymap binds no `PickCompare`, so
        // this screen is unreachable here. Rendering the meter keeps the
        // fallback honest if that ever changes.
        Screen::Meter | Screen::Compare => draw_meter(frame, body, app),
    }
    draw_footer(frame, footer, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &ClientState) {
    let left = match app.screen {
        Screen::List => match app.segment_count() {
            0 => "no segments".to_string(),
            n => format!("{n} segments"),
        },
        Screen::Meter | Screen::Compare => match app.segment_name() {
            Some(name) => {
                let overall = app.segment_kind() == Some(SegmentKind::Overall);
                // R10: a keyed visit's overall reads timed/depleted, with
                // the earned tier or overtime when the par timers are known.
                // A known key outcome beats LIVE, like the overlay.
                let key = app
                    .segment_pars_ms()
                    .map(|pars| fmt::key_tag(app.duration_ms(), pars, app.segment_success()));
                let state = match key {
                    Some(tag) if app.segment_success().is_some() => tag,
                    Some(tag) if app.is_live() => format!("LIVE {tag}"),
                    _ if app.is_live() => "LIVE".to_string(),
                    _ => match (app.segment_success(), overall) {
                        // R13: arena matches word the home team's outcome.
                        (Some(true), false) if app.segment_arena() => "Win",
                        (Some(false), false) if app.segment_arena() => "Loss",
                        (Some(true), false) => "Kill",
                        (Some(false), false) => "Wipe",
                        (Some(true), true) => "Timed",
                        (Some(false), true) => "Over",
                        (None, _) => "Done",
                    }
                    .to_string(),
                };
                let mut s = format!(
                    "[{}/{}] {}  {}  {}",
                    app.segment_index() + 1,
                    app.segment_count(),
                    name,
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
        },
    };
    let mid = app
        .source
        .clone()
        .unwrap_or_else(|| "waiting for log file".to_string());
    let right = match app.screen {
        Screen::List => "Segments",
        Screen::Meter | Screen::Compare => view_name(app.view),
    };
    let text = compose(&left, &mid, right, area.width as usize);
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

fn draw_footer(frame: &mut Frame, area: Rect, app: &ClientState) {
    let hints = match app.screen {
        Screen::List => LIST_HINTS,
        Screen::Meter if app.drill.is_some() => DRILL_HINTS,
        Screen::Meter | Screen::Compare => METER_HINTS,
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

/// The segment list: one row per indexed or live segment, newest last.
fn draw_list(frame: &mut Frame, area: Rect, app: &ClientState) {
    let rows = app.list_rows();
    let height = area.height as usize;
    if height == 0 || area.width == 0 {
        return;
    }
    if rows.is_empty() {
        let text = if app.source.is_some() {
            "No segments in this log yet."
        } else {
            "Waiting for a combat log."
        };
        frame.render_widget(
            Paragraph::new(Line::from(text))
                .alignment(Alignment::Center)
                .style(Style::new().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let sel = app.list_selection().min(rows.len() - 1);
    let offset = sel.saturating_sub(height - 1);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, row)| {
            let selected = i == sel;
            let text = list_row_text(i + 1, row, area.width as usize, selected);
            if selected {
                Line::from(text).style(Style::new().fg(Color::Black).bg(Color::Cyan))
            } else if row.live {
                Line::from(text).style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            } else if row.kind == SegmentKind::Trash {
                // Details-style: trash is disposable, bosses are precious.
                Line::from(text).style(Style::new().fg(Color::DarkGray))
            } else {
                Line::from(text)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// ` > 12  Midnight Falls        Kill   1:03  21:00`, dropping columns
/// right-to-left as the terminal narrows.
fn list_row_text(rank: usize, row: &ListRow, width: usize, selected: bool) -> String {
    // R10: a keyed visit's Σ row carries the tier / overtime detail; a
    // known key outcome beats LIVE.
    let key = row
        .pars_ms
        .map(|pars| fmt::key_tag(row.duration_ms, pars, row.success));
    let state = match key {
        Some(tag) if row.success.is_some() => tag,
        Some(tag) if row.live => format!("LIVE {tag}"),
        _ if row.live => "LIVE".to_string(),
        _ => match (row.kind, row.success) {
            // R10: a keyed visit's overall reads timed/depleted.
            (SegmentKind::Overall, Some(true)) => "Time",
            (SegmentKind::Overall, Some(false)) => "Over",
            // R13: arena matches word the home team's outcome.
            (_, Some(true)) if row.arena => "Win",
            (_, Some(false)) if row.arena => "Loss",
            (_, Some(true)) => "Kill",
            (_, Some(false)) => "Wipe",
            (_, None) => "-",
        }
        .to_string(),
    };
    let hh = (row.start_ms / 3_600_000).rem_euclid(24);
    let mm = (row.start_ms / 60_000).rem_euclid(60);

    // R10: the Overall header row wears a Σ so it can't be mistaken for a
    // fight with the instance's name.
    let name = match row.kind {
        SegmentKind::Overall => format!("Σ {}", row.name),
        _ => row.name.clone(),
    };
    // The name absorbs whatever the dropped right-hand columns free up.
    let reserved = 6
        + if width >= 30 { 6 } else { 0 }
        + if width >= 40 { 8 } else { 0 }
        + if width >= 48 { 7 } else { 0 };
    let name_w = (width.saturating_sub(reserved)).clamp(6, 40);
    let mut s = format!(
        "{sel}{rank:>3}  {name:<name_w$}",
        sel = if selected { '>' } else { ' ' },
        name = truncate(&name, name_w),
    );
    if width >= 30 {
        s.push_str(&format!("  {state:<4}"));
    }
    if width >= 40 {
        s.push_str(&format!("  {:>6}", duration(row.duration_ms)));
    }
    if width >= 48 {
        s.push_str(&format!("  {hh:02}:{mm:02}"));
    }
    truncate(&s, width)
}

fn draw_meter(frame: &mut Frame, area: Rect, app: &ClientState) {
    let rows = app.rows();
    if rows.is_empty() {
        return draw_empty(frame, area, app.view);
    }
    draw_rows(frame, area, &rows, app.row_sel, true, app.view);
}

fn draw_drilldown(frame: &mut Frame, area: Rect, app: &ClientState) {
    let Some(drill) = app.drill.as_ref() else {
        return;
    };
    // v16: the ability drill — the TUI words the stats the by-spell row
    // carries (the graph is the GUI's; here the numbers are the story).
    if let Some((_, spell_label)) = app.drill_spell() {
        let title = format!(" {} ▸ {} ", drill.label, spell_label);
        let block = Block::bordered()
            .title(title)
            .border_style(Style::new().fg(Color::Cyan));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let body = match app.drill_spell_row() {
            Some(r) => {
                let avg = match r.amount.checked_div(r.count) {
                    Some(v) if r.count > 0 => human(v),
                    _ => "—".to_string(),
                };
                let extra = if r.extra > 0 {
                    format!("   extra {}", human(r.extra))
                } else {
                    String::new()
                };
                format!(
                    "total {}   share {:.1}%   hits {}   crit {:.0}%   avg {}{}",
                    human(r.amount),
                    r.pct,
                    human(r.count),
                    r.crit_pct(),
                    avg,
                    extra,
                )
            }
            None => "no data yet".to_string(),
        };
        frame.render_widget(Paragraph::new(Line::from(body)), inner);
        return;
    }
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
    // Not `rows.first()`: the death-recap pane (R9) is chronological, so the
    // biggest amount can sit anywhere in the list.
    let max = rows.iter().map(|r| r.amount).max().unwrap_or(0).max(1);
    let any_extra = extra_tag(view).is_some() && rows.iter().any(|r| r.extra > 0);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, row)| {
            let selected = i == sel;
            let (pre, bar, post) = row_parts(
                i + 1,
                row,
                max,
                area.width as usize,
                selected && focused,
                view,
                any_extra,
            );
            match (selected, focused) {
                // The selection block overrides everything for readability.
                (true, true) => Line::from(format!("{pre}{bar}{post}"))
                    .style(Style::new().fg(Color::Black).bg(Color::Cyan)),
                _ => {
                    let base = if selected {
                        Style::new().add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    };
                    // The bar carries the player's class color; colorless until a
                    // COMBATANT_INFO names the spec.
                    let bar_style = match row.class {
                        Some(class) => {
                            let (r, g, b) = class.rgb();
                            base.fg(Color::Rgb(r, g, b))
                        }
                        None => base,
                    };
                    // R13: the enemy team's names read red — with the teams
                    // grouped by the sort, that splits the chart visually.
                    let pre_style = if row.enemy { base.fg(Color::Red) } else { base };
                    Line::from(vec![
                        Span::styled(pre, pre_style),
                        Span::styled(bar, bar_style),
                        Span::styled(post, pre_style),
                    ])
                }
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

struct Cols {
    name: usize,
    bar: usize,
    amount: usize,
    extra: usize,
    rate: usize,
    pct: usize,
}

/// Columns drop right-to-left as the terminal narrows; the bar absorbs slack.
/// The overkill/overheal column is only worth its width when something in this
/// view actually has some — most damage rows never do.
fn columns(width: usize, any_extra: bool) -> Cols {
    let name = (width / 3).clamp(6, 20).min(width);
    let pct = if width >= 30 { 6 } else { 0 };
    let rate = if width >= 60 { 9 } else { 0 };
    let amount = if width >= 44 { 9 } else { 0 };
    let extra = if width >= 100 && any_extra { 9 } else { 0 };
    let fixed = 4
        + name
        + 1
        + if amount > 0 { amount + 1 } else { 0 }
        + if extra > 0 { extra + 1 } else { 0 }
        + if rate > 0 { rate + 1 } else { 0 }
        + if pct > 0 { pct + 1 } else { 0 };
    Cols {
        name,
        bar: width.saturating_sub(fixed),
        amount,
        extra,
        rate,
        pct,
    }
}

/// What `Row::extra` means for the current view: damage wasted on an already
/// dead target, or healing that landed on a full health bar.
fn extra_tag(view: View) -> Option<&'static str> {
    match view {
        View::Damage => Some("ok"),
        View::Healing => Some("oh"),
        _ => None,
    }
}

/// The three renderable pieces of a meter row: text before the bar, the bar
/// itself (padded to its column), and the numeric columns after it. Split out
/// so the bar can carry a class color as its own span.
fn row_parts(
    rank: usize,
    row: &Row,
    max: u64,
    width: usize,
    selected: bool,
    view: View,
    any_extra: bool,
) -> (String, String, String) {
    let c = columns(width, any_extra);
    let filled = if c.bar == 0 {
        0
    } else {
        (row.amount as u128 * c.bar as u128 / max as u128) as usize
    };

    let pre = format!(
        "{}{rank:>2} {:<name$}",
        if selected { '>' } else { ' ' },
        truncate(&row.label, c.name),
        name = c.name,
    );
    let bar = if c.bar > 0 {
        let bar = "█".repeat(filled.min(c.bar));
        format!(" {bar:<w$}", w = c.bar)
    } else {
        String::new()
    };

    let mut post = String::new();
    if c.amount > 0 {
        post.push_str(&format!(" {:>w$}", human(row.amount), w = c.amount));
    }
    if c.extra > 0 {
        let extra = match extra_tag(view) {
            Some(tag) if row.extra > 0 => format!("{tag} {}", human(row.extra)),
            _ => String::new(),
        };
        post.push_str(&format!(" {extra:>w$}", w = c.extra));
    }
    if c.rate > 0 {
        let rate = if is_rate_view(view) {
            human(row.per_sec as u64)
        } else {
            "-".to_string()
        };
        post.push_str(&format!(" {rate:>w$}", w = c.rate));
    }
    if c.pct > 0 {
        post.push_str(&format!(" {:>w$}", format!("{:.1}%", row.pct), w = c.pct));
    }

    // Same guard as the old single-string path: never overflow the width,
    // dropping from the right.
    let pre = truncate(&pre, width);
    let bar = truncate(&bar, width.saturating_sub(pre.chars().count()));
    let post = truncate(
        &post,
        width.saturating_sub(pre.chars().count() + bar.chars().count()),
    );
    (pre, bar, post)
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use wowdps_daemon::mock::{MockDaemon, pump};
    use wowdps_model::Action;
    use wowdps_proto::{ClientState, DaemonMsg};

    /// The rendered buffer as one string per row.
    fn render(app: &ClientState, width: u16, height: u16) -> Vec<String> {
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

    fn apply(state: &mut ClientState, mock: &mut MockDaemon, action: Action) {
        let reqs = state.apply(action);
        pump(state, mock, reqs);
    }

    /// Indexed startup over the whole fixture: the list screen.
    fn indexed_state() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The meter on the newest segment: the final wipe.
    fn wipe_state() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = indexed_state();
        apply(&mut state, &mut mock, Action::Open);
        (state, mock)
    }

    /// Mid-fight arrival: the live meter, no navigation needed.
    fn live_state() -> (ClientState, MockDaemon) {
        let mut mock = MockDaemon::fixture_live();
        let mut state = ClientState::new();
        let first = state.initial_request();
        pump(&mut state, &mut mock, vec![first]);
        (state, mock)
    }

    /// The fixture's second segment: the boss kill, with the richest data.
    fn kill_state() -> (ClientState, MockDaemon) {
        let (mut state, mut mock) = wipe_state();
        apply(&mut state, &mut mock, Action::OlderSegment);
        apply(&mut state, &mut mock, Action::OlderSegment);
        assert_eq!(state.segment_name().as_deref(), Some("The Ashen Warden"));
        (state, mock)
    }

    #[test]
    fn meter_view_shows_encounter_view_and_rows_in_order() {
        let (state, _mock) = live_state();
        let lines = render(&state, 100, 20);
        let all = flat(&lines);

        let name = state.segment_name().unwrap();
        assert!(all.contains(&name), "header names the encounter:\n{all}");
        assert!(all.contains("Damage"), "header names the view:\n{all}");
        assert!(all.contains("LIVE"), "live segment is marked:\n{all}");

        let rows = state.rows();
        assert_eq!(rows.len(), 3);
        let positions: Vec<usize> = rows.iter().map(|r| row_index(&lines, &r.label)).collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "rows must render in meter order (desc by amount):\n{all}"
        );
    }

    #[test]
    fn meter_rows_carry_amount_rate_and_pct() {
        let (state, _mock) = kill_state();
        let lines = render(&state, 100, 20);
        let top = &state.rows()[0];
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

    /// End-to-end: the numbers on screen are the validator's hand-computed
    /// expected values for `fixtures/sample.txt`, served over the protocol.
    #[test]
    fn the_rendered_numbers_are_the_expected_fixture_totals() {
        let (state, _mock) = kill_state();
        let lines = render(&state, 110, 20);
        let line = &lines[row_index(&lines, "Thraxx-Nebula-US")];
        // 185 370 damage, 3089.50 DPS, 50.83 % of 364 670, 5 200 overkill.
        for want in ["185.4k", "3.1k", "50.8%", "ok 5.2k"] {
            assert!(line.contains(want), "expected {want:?} in: {line:?}");
        }
    }

    #[test]
    fn healing_rows_show_overheal_as_the_extra_column() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::SetView(View::Healing));
        let lines = render(&state, 110, 20);
        let line = &lines[row_index(&lines, "Mírelle-Nebula-US")];
        // 149 800 effective healing with 27 300 overheal.
        for want in ["149.8k", "oh 27.3k"] {
            assert!(line.contains(want), "expected {want:?} in: {line:?}");
        }
    }

    #[test]
    fn the_extra_column_only_exists_when_something_fills_it() {
        assert_eq!(
            columns(110, false).extra,
            0,
            "no overkill anywhere: no column"
        );
        assert!(columns(110, true).extra > 0);
        assert!(
            columns(110, false).bar > columns(110, true).bar,
            "the bar reclaims the space"
        );
        assert_eq!(columns(80, true).extra, 0, "no room for it at 80 cols");
    }

    #[test]
    fn the_extra_column_is_dropped_on_narrow_terminals() {
        let (state, _mock) = kill_state();
        let line = &render(&state, 80, 20)[row_index(&render(&state, 80, 20), "Thraxx-Nebula-US")];
        assert!(
            !line.contains("ok "),
            "no room for overkill at 80 cols: {line:?}"
        );
        assert!(line.contains("185.4k"), "amount still shown: {line:?}");
    }

    #[test]
    fn the_selected_row_is_marked() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::Down);
        let lines = render(&state, 100, 20);
        let selected = state.rows()[1].label.clone();
        let line = &lines[row_index(&lines, &selected)];
        assert!(
            line.trim_start().starts_with('>'),
            "selection marker missing: {line:?}"
        );
    }

    #[test]
    fn count_views_render_a_dash_instead_of_a_rate() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::SetView(View::Interrupts));
        let rows = state.rows();
        assert!(!rows.is_empty(), "the fixture has interrupts on this kill");
        let lines = render(&state, 100, 20);
        let line = &lines[row_index(&lines, &rows[0].label)];
        assert!(
            line.contains(" - "),
            "expected a dash for per_sec: {line:?}"
        );
    }

    #[test]
    fn drilldown_shows_both_panes_for_the_selected_player() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::Open);
        let drill = state.drill.clone().unwrap();
        let (by_spell, by_target) = state.breakdown();
        assert!(!by_spell.is_empty() && !by_target.is_empty());

        let lines = render(&state, 140, 20);
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
    fn a_pets_damage_is_labelled_in_its_owners_breakdown() {
        // Contract: pets roll into the owner's row, and only show up by name
        // inside the drilldown.
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::Down); // the hunter, who has a pet
        apply(&mut state, &mut mock, Action::Open);
        let all = flat(&render(&state, 140, 20));
        assert!(
            all.contains("Sharptooth"),
            "expected the pet named in the by-spell pane:\n{all}"
        );
        let (meter_state, _mock) = wipe_state();
        let meter = flat(&render(&meter_state, 140, 20));
        assert!(
            !meter.contains("Sharptooth"),
            "but never as a meter row of its own:\n{meter}"
        );
    }

    #[test]
    fn drilldown_marks_the_focused_pane_selection_only() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::Open);
        apply(&mut state, &mut mock, Action::Down);
        let (by_spell, _) = state.breakdown();
        let lines = render(&state, 140, 20);
        let line = &lines[row_index(&lines, &by_spell[1].label)];
        assert!(
            line.contains('>'),
            "focused pane selection missing: {line:?}"
        );
    }

    #[test]
    fn a_finished_segment_shows_its_result_not_live() {
        let (state, _mock) = kill_state();
        let all = flat(&render(&state, 100, 20));
        assert!(all.contains("The Ashen Warden"), "{all}");
        assert!(!all.contains("LIVE"), "closed segment is not live:\n{all}");
        assert!(all.contains("Kill"), "kill/wipe result missing:\n{all}");
    }

    #[test]
    fn a_wipe_is_labelled_as_one() {
        let (state, _mock) = wipe_state();
        let all = flat(&render(&state, 100, 20));
        assert!(all.contains("Verkath the Hollow"), "{all}");
        assert!(all.contains("Wipe"), "wipe result missing:\n{all}");
    }

    #[test]
    fn an_empty_view_says_so_instead_of_rendering_nothing() {
        let (mut state, mut mock) = wipe_state();
        apply(&mut state, &mut mock, Action::SetView(View::Deaths));
        // Walk back to position 1, the opening trash pull (position 0 is
        // the visit's Overall now, and the raid's deaths land there too).
        while state.segment_index() > 1 {
            apply(&mut state, &mut mock, Action::OlderSegment);
        }
        assert!(state.rows().is_empty(), "the opening trash had no deaths");
        let all = flat(&render(&state, 100, 20));
        assert!(
            all.to_lowercase().contains("no "),
            "expected an empty-state message:\n{all}"
        );
    }

    #[test]
    fn the_footer_documents_the_keybinds() {
        let (state, _mock) = wipe_state();
        let all = flat(&render(&state, 120, 20));
        for hint in ["d", "h", "i", "c", "x", "K", "[", "]", "q"] {
            assert!(all.contains(hint), "footer missing {hint:?}:\n{all}");
        }
    }

    #[test]
    fn an_empty_client_renders_the_startup_state() {
        let state = ClientState::new();
        let all = flat(&render(&state, 100, 20));
        assert!(all.to_lowercase().contains("waiting"), "{all}");
        assert!(all.contains("no segments"), "{all}");
    }

    #[test]
    fn the_list_screen_shows_every_segment_with_result_and_duration() {
        let (state, _mock) = indexed_state();
        let lines = render(&state, 100, 20);
        let all = flat(&lines);

        assert!(all.contains("5 segments"), "header counts them:\n{all}");
        assert!(all.contains("Segments"), "header names the screen:\n{all}");
        assert!(all.contains("sample.txt"), "header names the file:\n{all}");
        // R10: the fixture's raid visit heads the list as its Overall row.
        assert!(all.contains("Σ Sepulcher of the Ashen Vow"), "{all}");
        assert!(all.contains("The Ashen Warden"), "{all}");
        assert!(all.contains("Verkath the Hollow"), "{all}");
        // Trash pulls are named after their dominant enemy, Details-style.
        assert!(all.contains("Gloomstalker"), "{all}");
        assert!(all.contains("Hollow Drudge"), "{all}");
        assert!(all.contains("Kill"), "the kill reads as one:\n{all}");
        assert!(all.contains("Wipe"), "and the wipe too:\n{all}");
        assert!(all.contains("1:00"), "the kill's duration:\n{all}");
        assert!(all.contains("0:45"), "the wipe's duration:\n{all}");
        assert!(all.contains("enter open"), "list footer hints:\n{all}");
        assert!(
            !all.contains('█'),
            "no meter bars: nothing was loaded for the list:\n{all}"
        );

        let order = ["The Ashen Warden", "Verkath the Hollow"].map(|n| row_index(&lines, n));
        assert!(order[0] < order[1], "oldest first:\n{all}");
    }

    #[test]
    fn the_selected_list_row_is_marked() {
        let (state, _mock) = indexed_state();
        let lines = render(&state, 100, 20);
        // Startup selects the newest segment: the final wipe.
        let line = &lines[row_index(&lines, "Verkath the Hollow")];
        assert!(
            line.trim_start().starts_with('>'),
            "selection marker missing: {line:?}"
        );
    }

    #[test]
    fn an_open_fight_is_listed_as_live() {
        // Arrive mid-fight, then back out to the list: the open fight's row
        // carries the LIVE marker.
        let (mut state, mut mock) = live_state();
        apply(&mut state, &mut mock, Action::Back);
        let lines = render(&state, 100, 20);
        let line = &lines[row_index(&lines, "Verkath the Hollow")];
        assert!(line.contains("LIVE"), "open fight marked live: {line:?}");
    }

    #[test]
    fn the_list_survives_narrow_terminals() {
        let (state, _mock) = indexed_state();
        for (w, h) in [(1, 1), (10, 3), (24, 10), (40, 5), (200, 60)] {
            render(&state, w, h);
        }
        let narrow = flat(&render(&state, 24, 10));
        assert!(
            narrow.contains("Ashen"),
            "names survive narrowing:\n{narrow}"
        );
    }

    #[test]
    fn daemon_errors_surface_in_the_footer() {
        let (mut state, _mock) = wipe_state();
        let _ = state.on_msg(DaemonMsg::Fatal("permission denied".into()));
        let all = flat(&render(&state, 100, 20));
        assert!(all.contains("permission denied"), "{all}");
    }

    #[test]
    fn tiny_terminals_render_without_panicking() {
        const SIZES: [(u16, u16); 6] = [(1, 1), (4, 2), (20, 3), (39, 10), (59, 8), (200, 60)];
        let (wipe, _m1) = wipe_state();
        for state in [ClientState::new(), wipe] {
            for (w, h) in SIZES {
                render(&state, w, h);
            }
        }
        let (mut drilled, mut mock) = kill_state();
        apply(&mut drilled, &mut mock, Action::Open);
        for (w, h) in SIZES {
            render(&drilled, w, h);
        }
    }

    #[test]
    fn long_row_lists_scroll_to_keep_the_selection_visible() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::Down); // the hunter: most spells
        apply(&mut state, &mut mock, Action::Open);
        let (by_spell, _) = state.breakdown();
        // A pane four rows tall cannot show them all at once.
        assert!(by_spell.len() > 4, "got {} spells", by_spell.len());
        for _ in 0..by_spell.len() {
            apply(&mut state, &mut mock, Action::Down);
        }
        let last = by_spell.last().unwrap().label.clone();
        let all = flat(&render(&state, 140, 8));
        assert!(
            all.contains(&last),
            "selection scrolled out of view:\n{all}"
        );
    }

    use wowdps_model::{SegmentId, SegmentInfo};
    use wowdps_proto::SegmentRef;

    /// v16: the ability drill words the by-spell row's stats in a titled
    /// box — and says so when the drilled snapshot has not arrived.
    #[test]
    fn the_ability_drill_renders_its_stats_or_says_no_data() {
        let (mut state, mut mock) = kill_state();
        apply(&mut state, &mut mock, Action::Open);
        apply(&mut state, &mut mock, Action::Open);
        let (key, label) = state.drill_spell().cloned().expect("ability drill open");
        let row = state.drill_spell_row().expect("its row");
        assert_eq!(row.key, key);
        let all = flat(&render(&state, 120, 20));
        assert!(all.contains(&format!("▸ {label}")), "{all}");
        assert!(
            all.contains(&format!("total {}", human(row.amount))),
            "{all}"
        );
        assert!(all.contains(&format!("hits {}", human(row.count))), "{all}");
        assert!(all.contains("crit"), "{all}");
        assert!(
            !all.contains("By target"),
            "the ability view replaces the panes:\n{all}"
        );

        // An ability the breakdown does not carry: nothing to word yet.
        state.drill.as_mut().unwrap().spell = Some(("nope".to_string(), "Nope".to_string()));
        let all = flat(&render(&state, 120, 20));
        assert!(all.contains("no data yet"), "{all}");
    }

    /// The list wording: keyed overalls read timed/depleted, arenas win/
    /// loss, a known key tier beats LIVE, and an open key reads LIVE + tier.
    #[test]
    fn list_rows_word_every_outcome() {
        let base = ListRow {
            kind: SegmentKind::Encounter,
            name: "X".to_string(),
            start_ms: 3_600_000 * 25 + 60_000 * 7,
            success: None,
            duration_ms: 90_000,
            live: false,
            instance: None,
            pars_ms: None,
            arena: false,
            encounter: None,
        };
        let text = |row: &ListRow| list_row_text(1, row, 60, false);
        assert!(text(&base).contains("  -   "), "{}", text(&base));
        assert!(text(&base).contains("01:07"), "hour wraps: {}", text(&base));
        let overall = |success| ListRow {
            kind: SegmentKind::Overall,
            success,
            ..base.clone()
        };
        assert!(text(&overall(Some(true))).contains("Time"));
        assert!(text(&overall(Some(false))).contains("Over"));
        assert!(text(&overall(Some(false))).contains("Σ X"));
        let arena = |success| ListRow {
            arena: true,
            encounter: None,
            success,
            ..base.clone()
        };
        assert!(text(&arena(Some(true))).contains("Win"));
        assert!(text(&arena(Some(false))).contains("Loss"));
        let keyed = |success, live| ListRow {
            kind: SegmentKind::Overall,
            success,
            live,
            pars_ms: Some((100_000, 80_000, 60_000)),
            ..base.clone()
        };
        let done = text(&keyed(Some(true), false));
        assert!(!done.contains("LIVE") && !done.contains("Time"), "{done}");
        let open = text(&keyed(None, true));
        assert!(open.contains("LIVE "), "{open}");
        // Narrow: only the name column survives.
        assert!(!list_row_text(1, &base, 20, true).contains("1:30"));
    }

    fn header_state(info: SegmentInfo) -> ClientState {
        let mut state = ClientState::new();
        let _ = state.on_msg(DaemonMsg::SegmentList {
            seq: 1,
            entries: Vec::new(),
            source: Some("x.txt".to_string()),
            active: true,
        });
        assert_eq!(state.screen, Screen::Meter);
        let _ = state.on_msg(DaemonMsg::Snapshot {
            seq: 2,
            segment: SegmentRef::Live,
            id: Some(SegmentId(1)),
            view: View::Damage,
            info,
            rows: Vec::new(),
            total_rows: 0,
            breakdown: None,
            segment_count: 1,
            source: Some("x.txt".to_string()),
            status: None,
        });
        state
    }

    /// The meter header's state word, for the shapes the fixture cannot
    /// produce: keyed overalls, arena matches, an open key.
    #[test]
    fn the_header_words_keyed_overalls_and_arenas() {
        let info = |kind, success, live, pars_ms, arena| SegmentInfo {
            kind,
            name: "Hall".to_string(),
            start_ms: 0,
            duration_ms: 70_000,
            success,
            live,
            instance: Some(0),
            pars_ms,
            arena,
            encounter: None,
        };
        let header = |st: &ClientState| render(st, 100, 5)[0].clone();
        let pars = Some((100_000, 80_000, 60_000));
        let timed = header_state(info(SegmentKind::Overall, Some(true), false, pars, false));
        let h = header(&timed);
        assert!(!h.contains("LIVE") && h.contains("Hall"), "{h}");
        let open_key = header_state(info(SegmentKind::Overall, None, true, pars, false));
        assert!(header(&open_key).contains("LIVE "), "{}", header(&open_key));
        let untimed = header_state(info(SegmentKind::Overall, Some(true), false, None, false));
        assert!(header(&untimed).contains("Timed"), "{}", header(&untimed));
        let depleted = header_state(info(SegmentKind::Overall, Some(false), false, None, false));
        assert!(header(&depleted).contains("Over"), "{}", header(&depleted));
        let win = header_state(info(SegmentKind::Encounter, Some(true), false, None, true));
        assert!(header(&win).contains("Win"), "{}", header(&win));
        let loss = header_state(info(SegmentKind::Encounter, Some(false), false, None, true));
        assert!(header(&loss).contains("Loss"), "{}", header(&loss));
        let done = header_state(info(SegmentKind::Trash, None, false, None, false));
        assert!(header(&done).contains("Done"), "{}", header(&done));

        // On the meter with no snapshot at all, the header says so.
        let mut bare = ClientState::new();
        bare.screen = Screen::Meter;
        assert!(header(&bare).contains("no segments"), "{}", header(&bare));
    }

    #[test]
    fn a_log_with_no_segments_yet_is_told_apart_from_no_log() {
        let mut state = ClientState::new();
        let _ = state.on_msg(DaemonMsg::SegmentList {
            seq: 1,
            entries: Vec::new(),
            source: Some("fresh.txt".to_string()),
            active: false,
        });
        let all = flat(&render(&state, 80, 10));
        assert!(all.contains("No segments in this log yet."), "{all}");
    }

    /// A drilldown pane with nothing in it says so instead of going blank.
    #[test]
    fn an_empty_drilldown_pane_is_labelled() {
        let (mut state, mut mock) = wipe_state();
        apply(&mut state, &mut mock, Action::SetView(View::Deaths));
        assert!(!state.rows().is_empty(), "the wipe has deaths");
        apply(&mut state, &mut mock, Action::Open);
        let (_, by_target) = state.breakdown();
        let all = flat(&render(&state, 140, 20));
        if by_target.is_empty() {
            assert!(all.contains("No deaths recorded"), "{all}");
        }
        assert!(all.contains("By spell"), "{all}");
    }

    /// Widths too small for a bar or a name column still render something.
    #[test]
    fn a_meter_narrower_than_its_columns_still_renders() {
        let (state, _mock) = kill_state();
        let lines = render(&state, 8, 6);
        assert!(lines.iter().any(|l| l.contains('>')), "{}", flat(&lines));
        assert!(!flat(&lines).contains('█'), "no room for a bar");
        render(&state, 0, 0);
    }

    /// Hand-fed drill snapshots for the shapes the fixture never produces:
    /// an ability with no hits (no average to show), rows without a class
    /// color, and an empty target pane.
    #[test]
    fn drill_edge_shapes_render_their_placeholders() {
        use wowdps_model::{Drill, Pane};
        use wowdps_proto::Breakdown;
        let mut state = ClientState::new();
        let _ = state.on_msg(DaemonMsg::SegmentList {
            seq: 1,
            entries: Vec::new(),
            source: Some("x.txt".to_string()),
            active: true,
        });
        assert_eq!(state.screen, Screen::Meter);
        state.drill = Some(Drill {
            key: "P".to_string(),
            label: "Pat".to_string(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
            spell: None,
        });
        let plain = |key: &str| Row {
            key: key.to_string(),
            label: key.to_string(),
            amount: 10,
            ..Row::default()
        };
        let _ = state.on_msg(DaemonMsg::Snapshot {
            seq: 2,
            segment: SegmentRef::Live,
            id: Some(SegmentId(1)),
            view: View::Damage,
            info: SegmentInfo {
                kind: SegmentKind::Trash,
                name: "Pull".to_string(),
                start_ms: 0,
                duration_ms: 1000,
                success: None,
                live: true,
                instance: None,
                pars_ms: None,
                arena: false,
                encounter: None,
            },
            rows: vec![plain("P")],
            total_rows: 1,
            breakdown: Some(Breakdown {
                by_spell: vec![plain("Idle"), plain("Other")],
                by_target: Vec::new(),
                timeline: None,
                spell_timeline: None,
                spell_targets: None,
            }),
            segment_count: 1,
            source: Some("x.txt".to_string()),
            status: None,
        });
        let all = flat(&render(&state, 120, 12));
        assert!(
            all.contains("No damage recorded"),
            "empty target pane:\n{all}"
        );
        assert!(all.contains("Other"), "uncolored, unselected row:\n{all}");

        state.drill.as_mut().unwrap().spell = Some(("Idle".to_string(), "Idle".to_string()));
        let all = flat(&render(&state, 120, 12));
        assert!(all.contains("avg —"), "no hits, no average:\n{all}");
        assert!(!all.contains("extra"), "no overkill column:\n{all}");
    }
}
