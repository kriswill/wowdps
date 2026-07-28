//! Application state: which view, which segment, which row, drilldown or not.
//! Pure logic — no drawing, no I/O — so all of it is unit-testable.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{Meter, Row, Segment, View, parse_line};
use crate::tail::TailEvent;

/// A key press translated into intent. Keeps the keymap testable on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SetView(View),
    OlderSegment,
    NewerSegment,
    Up,
    Down,
    Open,
    Back,
    SwapPane,
    Quit,
}

/// Which half of the drilldown has the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Spell,
    Target,
}

/// An open drilldown. Keyed by row key (player guid), never by index, so a
/// re-sort between frames can't silently switch which player you're inspecting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drill {
    pub key: String,
    pub label: String,
    pub pane: Pane,
    pub spell_sel: usize,
    pub target_sel: usize,
}

pub struct App {
    pub meter: Meter,
    pub view: View,
    pub row_sel: usize,
    pub drill: Option<Drill>,
    /// Max log timestamp seen. Drives live durations — never the wall clock,
    /// so replaying a finished file reports the real encounter length.
    pub now_ms: i64,
    /// Log file being followed, for the header.
    pub source: Option<String>,
    /// Last tail error, shown in the footer.
    pub status: Option<String>,
    pub quit: bool,
    seg_sel: usize,
    /// Selected segment is the newest one, so new segments auto-follow.
    follow_live: bool,
}

pub fn action_for(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('d') => Action::SetView(View::Damage),
        KeyCode::Char('h') => Action::SetView(View::Healing),
        KeyCode::Char('i') => Action::SetView(View::Interrupts),
        KeyCode::Char('c') => Action::SetView(View::CrowdControl),
        KeyCode::Char('x') => Action::SetView(View::Dispels),
        // Shift-K, because lowercase k is vim-style "move up".
        KeyCode::Char('K') => Action::SetView(View::Deaths),
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Char('[') | KeyCode::Left => Action::OlderSegment,
        KeyCode::Char(']') | KeyCode::Right => Action::NewerSegment,
        KeyCode::Enter => Action::Open,
        KeyCode::Esc => Action::Back,
        KeyCode::Tab | KeyCode::BackTab => Action::SwapPane,
        _ => return None,
    })
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let meter = Meter::new();
        let seg_sel = meter.current_index();
        Self {
            meter,
            view: View::Damage,
            row_sel: 0,
            drill: None,
            now_ms: 0,
            source: None,
            status: None,
            quit: false,
            seg_sel,
            follow_live: true,
        }
    }

    pub fn segment_index(&self) -> usize {
        self.seg_sel
    }

    pub fn segment_count(&self) -> usize {
        self.meter.segments().len()
    }

    pub fn following_live(&self) -> bool {
        self.follow_live
    }

    pub fn segment(&self) -> Option<&Segment> {
        self.meter.segments().get(self.seg_sel)
    }

    /// True when the selected segment is still accumulating.
    pub fn is_live(&self) -> bool {
        self.segment().is_some_and(|s| s.end_ms.is_none())
    }

    pub fn rows(&self) -> Vec<Row> {
        self.segment()
            .map(|s| s.rows(self.view))
            .unwrap_or_default()
    }

    pub fn breakdown(&self) -> (Vec<Row>, Vec<Row>) {
        match (self.segment(), self.drill.as_ref()) {
            (Some(seg), Some(drill)) => seg.breakdown(&drill.key, self.view),
            _ => (Vec::new(), Vec::new()),
        }
    }

    pub fn duration_ms(&self) -> i64 {
        self.segment()
            .map(|s| s.duration_ms(self.now_ms))
            .unwrap_or(0)
    }

    /// Feed one raw log line through the parser into the meter.
    pub fn feed_line(&mut self, line: &str) {
        if let Some(parsed) = parse_line(line) {
            self.now_ms = self.now_ms.max(parsed.ts_ms);
            self.meter.feed(parsed);
            self.sync_segments();
        }
    }

    pub fn on_tail(&mut self, event: TailEvent) {
        match event {
            TailEvent::Lines(lines) => {
                for line in lines {
                    self.feed_line(&line);
                }
            }
            TailEvent::Switched(path) => {
                // A different log file means a different session: start over.
                self.source = Some(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                );
                self.meter = Meter::new();
                self.now_ms = 0;
                self.drill = None;
                self.row_sel = 0;
                self.follow_live = true;
                self.status = None;
                self.sync_segments();
            }
            TailEvent::Waiting => self.source = None,
            TailEvent::Error(msg) => self.status = Some(msg),
        }
    }

    /// Keep the selected segment pinned to the newest one while following.
    pub fn sync_segments(&mut self) {
        let count = self.segment_count();
        if count == 0 {
            self.seg_sel = 0;
        } else if self.follow_live {
            self.seg_sel = count - 1;
        } else {
            self.seg_sel = self.seg_sel.min(count - 1);
        }
        self.clamp_selection();
    }

    /// Rows re-sort and disappear between frames; never point past the end.
    fn clamp_selection(&mut self) {
        let len = self.rows().len();
        self.row_sel = if len == 0 {
            0
        } else {
            self.row_sel.min(len - 1)
        };
        if let Some(drill) = self.drill.as_mut() {
            let (by_spell, by_target) = match self.meter.segments().get(self.seg_sel) {
                Some(seg) => seg.breakdown(&drill.key, self.view),
                None => (Vec::new(), Vec::new()),
            };
            drill.spell_sel = clamp_to(drill.spell_sel, by_spell.len());
            drill.target_sel = clamp_to(drill.target_sel, by_target.len());
        }
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::SetView(view) => {
                self.view = view;
                self.clamp_selection();
            }
            Action::OlderSegment => {
                self.seg_sel = self.seg_sel.saturating_sub(1);
                self.follow_live = self.seg_sel + 1 == self.segment_count();
                self.row_sel = 0;
                self.clamp_selection();
            }
            Action::NewerSegment => {
                let last = self.segment_count().saturating_sub(1);
                self.seg_sel = (self.seg_sel + 1).min(last);
                self.follow_live = self.seg_sel == last;
                self.row_sel = 0;
                self.clamp_selection();
            }
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
            Action::Open => self.open_drilldown(),
            Action::Back => self.drill = None,
            Action::SwapPane => {
                if let Some(drill) = self.drill.as_mut() {
                    drill.pane = match drill.pane {
                        Pane::Spell => Pane::Target,
                        Pane::Target => Pane::Spell,
                    };
                }
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let (len, cur) = match self.drill.as_ref() {
            None => (self.rows().len(), self.row_sel),
            Some(drill) => {
                let (by_spell, by_target) = self.breakdown();
                match drill.pane {
                    Pane::Spell => (by_spell.len(), drill.spell_sel),
                    Pane::Target => (by_target.len(), drill.target_sel),
                }
            }
        };
        if len == 0 {
            return;
        }
        let next = cur.saturating_add_signed(delta).min(len - 1);
        match self.drill.as_mut() {
            None => self.row_sel = next,
            Some(drill) => match drill.pane {
                Pane::Spell => drill.spell_sel = next,
                Pane::Target => drill.target_sel = next,
            },
        }
    }

    fn open_drilldown(&mut self) {
        if self.drill.is_some() {
            return;
        }
        let rows = self.rows();
        let Some(row) = rows.get(self.row_sel) else {
            return;
        };
        self.drill = Some(Drill {
            key: row.key.clone(),
            label: row.label.clone(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
        });
    }
}

fn clamp_to(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> Option<Action> {
        action_for(key(KeyCode::Char(c)))
    }

    #[test]
    fn every_view_has_a_key() {
        assert_eq!(ch('d'), Some(Action::SetView(View::Damage)));
        assert_eq!(ch('h'), Some(Action::SetView(View::Healing)));
        assert_eq!(ch('i'), Some(Action::SetView(View::Interrupts)));
        assert_eq!(ch('c'), Some(Action::SetView(View::CrowdControl)));
        assert_eq!(ch('x'), Some(Action::SetView(View::Dispels)));
        assert_eq!(ch('K'), Some(Action::SetView(View::Deaths)));
    }

    #[test]
    fn movement_and_control_keys() {
        assert_eq!(ch('j'), Some(Action::Down));
        assert_eq!(ch('k'), Some(Action::Up));
        assert_eq!(action_for(key(KeyCode::Down)), Some(Action::Down));
        assert_eq!(action_for(key(KeyCode::Up)), Some(Action::Up));
        assert_eq!(ch('['), Some(Action::OlderSegment));
        assert_eq!(ch(']'), Some(Action::NewerSegment));
        assert_eq!(action_for(key(KeyCode::Enter)), Some(Action::Open));
        assert_eq!(action_for(key(KeyCode::Esc)), Some(Action::Back));
        assert_eq!(action_for(key(KeyCode::Tab)), Some(Action::SwapPane));
        assert_eq!(ch('q'), Some(Action::Quit));
    }

    #[test]
    fn ctrl_c_quits_since_raw_mode_swallows_the_signal() {
        let e = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(action_for(e), Some(Action::Quit));
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(ch('z'), None);
        assert_eq!(action_for(key(KeyCode::F(5))), None);
    }

    #[test]
    fn starts_on_damage_pinned_to_the_live_segment() {
        let app = App::new();
        assert_eq!(app.view, View::Damage);
        assert_eq!(app.segment_index(), app.segment_count() - 1);
        assert!(app.following_live());
        assert!(app.is_live());
        assert!(app.drill.is_none());
    }

    #[test]
    fn switching_view_keeps_the_selection_in_range() {
        let mut app = App::new();
        app.apply(Action::Down);
        app.apply(Action::Down);
        assert_eq!(app.row_sel, 2);
        // Deaths has fewer rows than damage on the live segment.
        app.apply(Action::SetView(View::Deaths));
        assert!(app.row_sel < app.rows().len().max(1));
        assert!(app.row_sel <= 2);
    }

    #[test]
    fn selection_stops_at_both_ends() {
        let mut app = App::new();
        for _ in 0..20 {
            app.apply(Action::Down);
        }
        assert_eq!(app.row_sel, app.rows().len() - 1);
        for _ in 0..20 {
            app.apply(Action::Up);
        }
        assert_eq!(app.row_sel, 0);
    }

    #[test]
    fn cycling_segments_unpins_and_repins_live() {
        let mut app = App::new();
        let last = app.segment_count() - 1;

        app.apply(Action::OlderSegment);
        assert_eq!(app.segment_index(), last - 1);
        assert!(!app.following_live(), "stepping back unpins from live");

        app.apply(Action::OlderSegment);
        assert_eq!(app.segment_index(), 0);
        app.apply(Action::OlderSegment);
        assert_eq!(app.segment_index(), 0, "clamped at the oldest segment");

        app.apply(Action::NewerSegment);
        app.apply(Action::NewerSegment);
        assert_eq!(app.segment_index(), last);
        assert!(app.following_live(), "reaching the newest re-pins");
        app.apply(Action::NewerSegment);
        assert_eq!(app.segment_index(), last);
    }

    #[test]
    fn sync_follows_the_newest_segment_only_while_pinned() {
        let mut app = App::new();
        let last = app.segment_count() - 1;

        app.apply(Action::OlderSegment);
        let pinned_to = app.segment_index();
        app.sync_segments();
        assert_eq!(app.segment_index(), pinned_to, "history stays put");

        app.apply(Action::NewerSegment);
        app.sync_segments();
        assert_eq!(app.segment_index(), last);
    }

    #[test]
    fn enter_opens_the_drilldown_for_the_selected_row() {
        let mut app = App::new();
        app.apply(Action::Down);
        let expected = app.rows()[1].clone();

        app.apply(Action::Open);
        let drill = app.drill.as_ref().expect("drilldown opened");
        assert_eq!(drill.key, expected.key);
        assert_eq!(drill.label, expected.label);
        assert_eq!(drill.pane, Pane::Spell);

        let (by_spell, by_target) = app.breakdown();
        assert!(!by_spell.is_empty());
        assert!(!by_target.is_empty());
    }

    #[test]
    fn esc_closes_the_drilldown() {
        let mut app = App::new();
        app.apply(Action::Open);
        assert!(app.drill.is_some());
        app.apply(Action::Back);
        assert!(app.drill.is_none());
        // Esc with nothing open is harmless.
        app.apply(Action::Back);
        assert!(app.drill.is_none());
    }

    #[test]
    fn enter_with_no_rows_does_nothing() {
        let mut app = App::new();
        app.apply(Action::SetView(View::Deaths));
        // Kel'Thuzad was a kill, so nobody died there.
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        assert!(app.rows().is_empty(), "expected an empty view to test");
        app.apply(Action::Open);
        assert!(app.drill.is_none());
    }

    #[test]
    fn drilldown_tracks_the_player_not_the_row_index() {
        let mut app = App::new();
        app.apply(Action::Down);
        app.apply(Action::Open);
        let who = app.drill.as_ref().unwrap().key.clone();

        // Feed enough to re-sort the live segment's rows underneath us.
        for i in 0..512 {
            app.feed_line(&format!(
                "7/26/2026 20:14:{:02}.000-4  SPELL_DAMAGE",
                i % 60
            ));
        }
        assert_eq!(app.drill.as_ref().unwrap().key, who);
        let (by_spell, _) = app.breakdown();
        assert!(!by_spell.is_empty(), "still resolving the same player");
    }

    #[test]
    fn tab_swaps_panes_and_each_keeps_its_own_selection() {
        let mut app = App::new();
        app.apply(Action::Open);
        app.apply(Action::Down);
        let drill = app.drill.as_ref().unwrap();
        assert_eq!(drill.spell_sel, 1);
        assert_eq!(drill.target_sel, 0);

        app.apply(Action::SwapPane);
        app.apply(Action::Down);
        let drill = app.drill.as_ref().unwrap();
        assert_eq!(drill.pane, Pane::Target);
        assert_eq!(drill.spell_sel, 1, "spell pane keeps its own cursor");
        assert_eq!(drill.target_sel, 1);
    }

    #[test]
    fn switching_to_a_view_with_fewer_breakdown_rows_clamps_the_panes() {
        let mut app = App::new();
        app.apply(Action::Open);
        app.apply(Action::Down);
        app.apply(Action::Down);
        app.apply(Action::SetView(View::Deaths));
        let (by_spell, _) = app.breakdown();
        let drill = app.drill.as_ref().unwrap();
        assert!(drill.spell_sel < by_spell.len().max(1));
    }

    #[test]
    fn a_new_log_file_resets_everything() {
        let mut app = App::new();
        app.apply(Action::Down);
        app.apply(Action::Open);
        app.apply(Action::OlderSegment);
        app.feed_line("7/26/2026 20:14:32.123-4  SPELL_DAMAGE");
        app.status = Some("stale".into());

        app.on_tail(TailEvent::Switched(PathBuf::from(
            "/logs/WoWCombatLog-02.txt",
        )));
        assert_eq!(app.source.as_deref(), Some("WoWCombatLog-02.txt"));
        assert_eq!(app.now_ms, 0);
        assert_eq!(app.row_sel, 0);
        assert!(app.drill.is_none());
        assert!(app.following_live());
        assert!(app.status.is_none());
    }

    #[test]
    fn tail_events_drive_the_header_and_footer() {
        let mut app = App::new();
        app.on_tail(TailEvent::Error("denied".into()));
        assert_eq!(app.status.as_deref(), Some("denied"));

        app.on_tail(TailEvent::Switched(PathBuf::from("/logs/a.txt")));
        app.on_tail(TailEvent::Waiting);
        assert!(app.source.is_none());
    }

    #[test]
    fn lines_advance_the_clock_and_the_live_duration() {
        let mut app = App::new();
        app.on_tail(TailEvent::Lines(vec![
            "7/26/2026 20:14:32.000-4  SPELL_DAMAGE".to_string(),
            "".to_string(),
        ]));
        let first = app.now_ms;
        assert!(first > 0, "timestamp picked up from the line");
        let early = app.duration_ms();

        app.on_tail(TailEvent::Lines(vec![
            "7/26/2026 20:16:32.000-4  SPELL_DAMAGE".to_string(),
        ]));
        assert!(app.now_ms > first);
        assert!(app.duration_ms() > early, "live segment duration ticks");
    }

    #[test]
    fn a_finished_segment_ignores_the_clock() {
        let mut app = App::new();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        let before = app.duration_ms();
        app.feed_line("7/26/2026 23:59:59.999-4  SPELL_DAMAGE");
        assert_eq!(app.duration_ms(), before);
    }

    #[test]
    fn quit_is_sticky() {
        let mut app = App::new();
        assert!(!app.quit);
        app.apply(Action::Quit);
        assert!(app.quit);
    }
}
