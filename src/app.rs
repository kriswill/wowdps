//! Application state: which screen, which segment, which row, drilldown or
//! not. Pure logic — no drawing, no I/O — so all of it is unit-testable. The
//! one I/O-shaped edge is lazy loading: `apply` records a `load_request`, and
//! `main.rs` services it with `index::load_segment` + `install_loaded`.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{Meter, Row, Segment, SegmentKind, SegmentMeta, View, parse_line};
use crate::tail::TailEvent;

/// A file younger than this at scan time means the game is still writing it,
/// so an open trailing segment is a fight in progress: skip the list and go
/// straight to its live meter.
const ACTIVE_FILE_MS: u64 = 10_000;

/// Parsed historical segments kept in memory. Each holds per-actor hashmaps,
/// so a whole night of them is needless weight; navigation reloads on demand.
const LOADED_CAP: usize = 8;

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

/// Which screen the app is on. It starts on the list; an in-progress fight
/// detected at startup jumps straight to its meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    List,
    Meter,
}

/// One row of the segment list: an indexed historical segment or a segment of
/// the live meter, presented uniformly for the UI.
#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
    pub kind: SegmentKind,
    pub name: String,
    pub start_ms: i64,
    pub success: Option<bool>,
    pub duration_ms: i64,
    pub live: bool,
}

pub struct App {
    /// The live meter, fed only the tail from the index's `live_offset`.
    pub meter: Meter,
    pub view: View,
    pub row_sel: usize,
    pub drill: Option<Drill>,
    /// Max log timestamp seen. Drives live durations — never the wall clock,
    /// so replaying a finished file reports the real encounter length.
    pub now_ms: i64,
    /// Log file being followed, for the header.
    pub source: Option<String>,
    /// Full path of that file; needed to lazily load slices.
    pub source_path: Option<PathBuf>,
    /// Last tail error, shown in the footer.
    pub status: Option<String>,
    pub quit: bool,
    pub screen: Screen,
    /// Closed historical segments from the scan, oldest first. The combined
    /// segment list the user navigates is this followed by the live meter's
    /// own segments.
    index: Vec<SegmentMeta>,
    /// Lazily parsed historical segments, FIFO-capped at [`LOADED_CAP`].
    /// Each entry is a meter fed exactly one segment's slice (plus seeds).
    loaded: Vec<(usize, Meter)>,
    /// An indexed segment the user asked for that is not loaded yet;
    /// `main.rs` services this between frames.
    load_pending: Option<usize>,
    /// Selection on the list screen, over `list_rows()`.
    list_sel: usize,
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
            source_path: None,
            status: None,
            quit: false,
            screen: Screen::List,
            index: Vec::new(),
            loaded: Vec::new(),
            load_pending: None,
            list_sel: 0,
            seg_sel,
            follow_live: true,
        }
    }

    pub fn segment_index(&self) -> usize {
        self.seg_sel
    }

    /// Indexed history plus the live meter's own segments.
    pub fn segment_count(&self) -> usize {
        self.index.len() + self.meter.segments().len()
    }

    pub fn following_live(&self) -> bool {
        self.follow_live
    }

    pub fn list_selection(&self) -> usize {
        self.list_sel
    }

    /// The segment list, oldest first: indexed history, then live segments.
    pub fn list_rows(&self) -> Vec<ListRow> {
        let indexed = self.index.iter().map(|m| ListRow {
            kind: m.kind,
            name: m.name.clone(),
            start_ms: m.start_ms,
            success: m.success,
            duration_ms: m.duration_ms,
            live: false,
        });
        let live = self.meter.segments().iter().map(|s| ListRow {
            kind: s.kind,
            name: s.name.clone(),
            start_ms: s.start_ms,
            success: s.success,
            duration_ms: s.duration_ms(self.now_ms),
            live: s.end_ms.is_none(),
        });
        indexed.chain(live).collect()
    }

    /// Resolve a combined position to segment data. Indexed positions resolve
    /// through the loaded cache — `None` until their slice has been parsed.
    fn segment_at(&self, pos: usize) -> Option<&Segment> {
        if pos < self.index.len() {
            let meter = &self.loaded.iter().find(|(i, _)| *i == pos)?.1;
            meter.segments().first()
        } else {
            self.meter.segments().get(pos - self.index.len())
        }
    }

    pub fn segment(&self) -> Option<&Segment> {
        self.segment_at(self.seg_sel)
    }

    /// Metadata for the selected segment when it is an indexed one.
    fn selected_meta(&self) -> Option<&SegmentMeta> {
        self.index.get(self.seg_sel)
    }

    /// True when the selected segment is still accumulating. Only a live
    /// segment can be: a lazily loaded historical slice may end without its
    /// closing event, but it is history all the same.
    pub fn is_live(&self) -> bool {
        self.seg_sel >= self.index.len()
            && self.segment().is_some_and(|s| s.end_ms.is_none())
    }

    /// Kill/wipe state for the header; the index knows it even when the
    /// loaded slice does not carry its own closing event.
    pub fn segment_success(&self) -> Option<bool> {
        match self.selected_meta() {
            Some(meta) => meta.success,
            None => self.segment().and_then(|s| s.success),
        }
    }

    pub fn segment_name(&self) -> Option<String> {
        match self.selected_meta() {
            Some(meta) => Some(meta.name.clone()),
            None => self.segment().map(|s| s.name.clone()),
        }
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
        // Indexed segments report the scanner's duration: the loaded slice may
        // lack its closing event, and the live clock must never stretch it.
        match self.selected_meta() {
            Some(meta) => meta.duration_ms,
            None => self
                .segment()
                .map(|s| s.duration_ms(self.now_ms))
                .unwrap_or(0),
        }
    }

    /// Feed one raw log line through the parser into the live meter.
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
                self.source_path = Some(path);
                self.meter = Meter::new();
                self.now_ms = 0;
                self.drill = None;
                self.row_sel = 0;
                self.follow_live = true;
                self.status = None;
                self.screen = Screen::List;
                self.index.clear();
                self.loaded.clear();
                self.load_pending = None;
                self.list_sel = 0;
                self.sync_segments();
            }
            TailEvent::Index { index, file_age_ms } => {
                // A fight in progress (open segment, file still being written)
                // skips the list: the meter is what you want mid-pull.
                let active = index.open.is_some()
                    && file_age_ms.is_some_and(|age| age < ACTIVE_FILE_MS);
                self.index = index.segments;
                self.list_sel = self.segment_count().saturating_sub(1);
                if active {
                    self.screen = Screen::Meter;
                    self.follow_live = true;
                    self.sync_segments();
                }
            }
            TailEvent::Waiting => self.source = None,
            TailEvent::Error(msg) => self.status = Some(msg),
        }
    }

    /// The indexed segment `main.rs` should load and `install_loaded`, if any.
    pub fn load_request(&self) -> Option<(usize, SegmentMeta)> {
        let pos = self.load_pending?;
        Some((pos, self.index.get(pos)?.clone()))
    }

    /// Hand over a meter fed with one indexed segment's slice. Completes the
    /// navigation that requested it.
    pub fn install_loaded(&mut self, pos: usize, meter: Meter) {
        self.loaded.retain(|(i, _)| *i != pos);
        self.loaded.push((pos, meter));
        if self.loaded.len() > LOADED_CAP {
            self.loaded.remove(0);
        }
        if self.load_pending == Some(pos) {
            self.load_pending = None;
            self.goto_segment(pos);
        }
    }

    /// The requested slice could not be read; stay where we are.
    pub fn load_failed(&mut self, msg: String) {
        self.load_pending = None;
        self.status = Some(msg);
    }

    /// Jump the meter screen to a combined position, loading lazily if the
    /// data is not there yet.
    fn goto_segment(&mut self, pos: usize) {
        let count = self.segment_count();
        if count == 0 {
            return;
        }
        let pos = pos.min(count - 1);
        if pos < self.index.len() && self.segment_at(pos).is_none() {
            self.load_pending = Some(pos);
            return;
        }
        self.screen = Screen::Meter;
        self.seg_sel = pos;
        self.follow_live = pos + 1 == count;
        self.row_sel = 0;
        self.drill = None;
        self.clamp_selection();
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
        if let Some(drill) = self.drill.as_ref() {
            let (by_spell, by_target) = match self.segment() {
                Some(seg) => seg.breakdown(&drill.key, self.view),
                None => (Vec::new(), Vec::new()),
            };
            let drill = self.drill.as_mut().expect("checked above");
            drill.spell_sel = clamp_to(drill.spell_sel, by_spell.len());
            drill.target_sel = clamp_to(drill.target_sel, by_target.len());
        }
    }

    pub fn apply(&mut self, action: Action) {
        match self.screen {
            Screen::List => self.apply_list(action),
            Screen::Meter => self.apply_meter(action),
        }
    }

    fn apply_list(&mut self, action: Action) {
        let count = self.segment_count();
        match action {
            Action::Quit => self.quit = true,
            Action::SetView(view) => self.view = view,
            Action::Up => self.list_sel = self.list_sel.saturating_sub(1),
            Action::Down => {
                if count > 0 {
                    self.list_sel = (self.list_sel + 1).min(count - 1);
                }
            }
            Action::Open => {
                if count > 0 {
                    self.goto_segment(self.list_sel.min(count - 1));
                }
            }
            _ => {}
        }
    }

    fn apply_meter(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::SetView(view) => {
                self.view = view;
                self.clamp_selection();
            }
            Action::OlderSegment => self.goto_segment(self.seg_sel.saturating_sub(1)),
            Action::NewerSegment => self.goto_segment(self.seg_sel + 1),
            Action::Up => self.move_selection(-1),
            Action::Down => self.move_selection(1),
            Action::Open => self.open_drilldown(),
            Action::Back => {
                if self.drill.is_some() {
                    self.drill = None;
                } else {
                    // Leave the meter for the list, cursor on this segment.
                    self.list_sel = self.seg_sel.min(self.segment_count().saturating_sub(1));
                    self.load_pending = None;
                    self.screen = Screen::List;
                }
            }
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

/// Replay raw lines into a fresh meter — the lazy-load path, shared with the
/// tests. Pure: no I/O, no clock.
pub fn meter_from_lines<'a, I: IntoIterator<Item = &'a str>>(lines: I) -> Meter {
    let mut meter = Meter::new();
    for line in lines {
        if let Some(parsed) = parse_line(line) {
            meter.feed(parsed);
        }
    }
    meter
}

fn clamp_to(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{fixture_app, fixture_app_live, fixture_lines};
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
    fn a_fresh_meter_has_nothing_to_show() {
        let app = App::new();
        assert_eq!(app.segment_count(), 0, "the real meter starts empty");
        assert!(app.segment().is_none());
        assert!(app.rows().is_empty());
        assert_eq!(app.duration_ms(), 0);
        assert!(!app.is_live());
        assert!(app.breakdown().0.is_empty());
    }

    #[test]
    fn starts_on_damage_pinned_to_the_live_segment() {
        let app = fixture_app_live();
        assert_eq!(app.view, View::Damage);
        assert_eq!(app.segment_count(), 4);
        assert_eq!(app.segment_index(), 3);
        assert!(app.following_live());
        assert!(app.is_live());
        assert!(app.drill.is_none());
    }

    #[test]
    fn switching_view_keeps_the_selection_in_range() {
        let mut app = fixture_app();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        assert_eq!(app.segment().unwrap().name, "The Ashen Warden");

        app.apply(Action::Down);
        app.apply(Action::Down);
        assert_eq!(app.row_sel, 2);

        // Only one player died on that kill, so the selection has to come back.
        app.apply(Action::SetView(View::Deaths));
        assert_eq!(app.rows().len(), 1);
        assert_eq!(app.row_sel, 0);
    }

    #[test]
    fn selection_stops_at_both_ends() {
        let mut app = fixture_app();
        assert!(app.rows().len() > 1);
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
        let mut app = fixture_app_live();
        let last = app.segment_count() - 1;
        assert!(last >= 2, "fixture gives us history to walk");

        app.apply(Action::OlderSegment);
        assert_eq!(app.segment_index(), last - 1);
        assert!(!app.following_live(), "stepping back unpins from live");

        for _ in 0..last + 2 {
            app.apply(Action::OlderSegment);
        }
        assert_eq!(app.segment_index(), 0, "clamped at the oldest segment");

        for _ in 0..last {
            app.apply(Action::NewerSegment);
        }
        assert_eq!(app.segment_index(), last);
        assert!(app.following_live(), "reaching the newest re-pins");
        app.apply(Action::NewerSegment);
        assert_eq!(app.segment_index(), last);
    }

    #[test]
    fn sync_follows_the_newest_segment_only_while_pinned() {
        let mut app = fixture_app_live();
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
    fn a_new_segment_opening_pulls_a_pinned_view_forward() {
        // Replay only the first encounter, then let the rest of the log arrive.
        let lines = fixture_lines();
        let cut = lines
            .iter()
            .position(|l| l.contains("ENCOUNTER_END"))
            .unwrap();
        let mut app = App::new();
        for line in &lines[..=cut] {
            app.feed_line(line);
        }
        let before = app.segment_count();
        assert_eq!(app.segment_index(), before - 1);

        for line in &lines[cut + 1..] {
            app.feed_line(line);
        }
        assert!(app.segment_count() > before, "more segments opened");
        assert_eq!(
            app.segment_index(),
            app.segment_count() - 1,
            "a pinned view follows them"
        );
    }

    #[test]
    fn enter_opens_the_drilldown_for_the_selected_row() {
        let mut app = fixture_app();
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
        let mut app = fixture_app();
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
        let mut app = fixture_app();
        app.apply(Action::SetView(View::Deaths));
        // Nobody died during the opening trash pull.
        for _ in 0..app.segment_count() {
            app.apply(Action::OlderSegment);
        }
        assert!(app.rows().is_empty(), "expected an empty view to test");
        app.apply(Action::Open);
        assert!(app.drill.is_none());
    }

    #[test]
    fn drilldown_tracks_the_player_not_the_row_index() {
        let mut app = fixture_app();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        app.apply(Action::Open); // the top damage row
        let who = app.drill.as_ref().unwrap().key.clone();
        assert_eq!(app.rows()[0].key, who);

        // Interrupts rank the same players in a different order.
        app.apply(Action::SetView(View::Interrupts));
        assert_ne!(
            app.rows()[0].key,
            who,
            "this view must re-order, or the test proves nothing"
        );
        assert_eq!(
            app.drill.as_ref().unwrap().key,
            who,
            "the drilldown follows the guid, not the row position"
        );
        assert!(!app.breakdown().0.is_empty(), "and still resolves them");
    }

    #[test]
    fn tab_swaps_panes_and_each_keeps_its_own_selection() {
        // The healer has several spells and several heal targets.
        let mut app = fixture_app();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        app.apply(Action::SetView(View::Healing));
        app.apply(Action::Open);
        let (by_spell, by_target) = app.breakdown();
        assert!(by_spell.len() > 1 && by_target.len() > 1);

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
        let mut app = fixture_app();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        app.apply(Action::Open);
        app.apply(Action::Down);
        app.apply(Action::Down);
        assert!(app.drill.as_ref().unwrap().spell_sel > 0);

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
        // Replay the fixture in two halves and watch the live clock move.
        let lines = fixture_lines();
        let cut = lines.len() / 2;
        let mut app = App::new();

        app.on_tail(TailEvent::Lines(lines[..cut].to_vec()));
        let first = app.now_ms;
        assert!(first > 0, "timestamp picked up from the log");
        assert!(app.is_live());
        let early = app.duration_ms();

        app.on_tail(TailEvent::Lines(lines[cut..].to_vec()));
        assert!(app.now_ms > first, "the clock is the log's, not the wall's");
        assert!(app.duration_ms() > early, "segment duration ticked on");
    }

    #[test]
    fn a_finished_segment_ignores_the_clock() {
        let mut app = fixture_app();
        app.apply(Action::OlderSegment);
        app.apply(Action::OlderSegment);
        assert_eq!(app.segment().unwrap().name, "The Ashen Warden");
        assert_eq!(app.duration_ms(), 60_000, "the encounter's own length");

        // Later lines must not stretch a closed encounter.
        app.now_ms += 10 * 60 * 1000;
        assert_eq!(app.duration_ms(), 60_000);
    }

    #[test]
    fn the_fixtures_encounters_report_their_gated_durations() {
        let app = fixture_app();
        let named: Vec<(String, i64)> = app
            .meter
            .segments()
            .iter()
            .map(|s| (s.name.clone(), s.duration_ms(app.now_ms)))
            .collect();
        assert!(named.contains(&("The Ashen Warden".to_string(), 60_000)));
        assert!(named.contains(&("Verkath the Hollow".to_string(), 45_000)));
    }

    #[test]
    fn quit_is_sticky() {
        let mut app = App::new();
        assert!(!app.quit);
        app.apply(Action::Quit);
        assert!(app.quit);
    }

    // ---- the indexed startup path ------------------------------------------

    use crate::testkit::fixture_app_indexed as indexed_app;

    /// What `main.rs` does between frames: load the requested slice.
    fn service_loads(app: &mut App) {
        for _ in 0..8 {
            let Some((pos, meta)) = app.load_request() else {
                return;
            };
            let path = app.source_path.clone().expect("switched to a file");
            let lines = crate::index::load_segment(&path, &meta).unwrap();
            app.install_loaded(pos, meter_from_lines(lines.iter().map(String::as_str)));
        }
        panic!("load requests should settle");
    }

    #[test]
    fn a_finished_log_starts_on_the_list_with_the_newest_selected() {
        let app = indexed_app();
        assert_eq!(app.screen, Screen::List);
        let rows = app.list_rows();
        assert_eq!(rows.len(), 4, "the fixture's four segments are listed");
        assert_eq!(app.list_selection(), 3);
        assert!(rows.iter().all(|r| !r.live), "everything is history");
        assert_eq!(rows[1].name, "The Ashen Warden");
        assert_eq!(rows[1].success, Some(true));
        assert_eq!(rows[1].duration_ms, 60_000);
        assert!(
            app.rows().is_empty(),
            "nothing was parsed beyond the index yet"
        );
    }

    #[test]
    fn opening_a_listed_segment_loads_it_lazily_and_lands_on_the_meter() {
        let mut app = indexed_app();
        app.apply(Action::Open);
        assert!(app.load_request().is_some(), "slice must be requested");
        assert_eq!(app.screen, Screen::List, "not on the meter until loaded");

        service_loads(&mut app);
        assert_eq!(app.screen, Screen::Meter);
        assert_eq!(app.segment_index(), 3);
        assert_eq!(app.segment_name().as_deref(), Some("Verkath the Hollow"));
        assert_eq!(app.segment_success(), Some(false), "the wipe reads as one");
        assert_eq!(app.duration_ms(), 45_000);
        assert!(!app.is_live(), "history is never LIVE");
        assert!(!app.rows().is_empty(), "the lazily parsed rows are there");
    }

    #[test]
    fn bracket_navigation_walks_history_loading_as_it_goes() {
        let mut app = indexed_app();
        app.apply(Action::Open);
        service_loads(&mut app);
        assert_eq!(app.segment_index(), 3);

        app.apply(Action::OlderSegment);
        service_loads(&mut app);
        assert_eq!(app.segment_index(), 2);
        assert!(!app.following_live());

        app.apply(Action::NewerSegment);
        assert_eq!(app.segment_index(), 3, "already cached: no reload needed");
        assert!(app.following_live());
    }

    #[test]
    fn esc_on_the_meter_returns_to_the_list() {
        let mut app = indexed_app();
        app.apply(Action::Open);
        service_loads(&mut app);
        assert_eq!(app.screen, Screen::Meter);

        app.apply(Action::Open); // drill in
        assert!(app.drill.is_some());
        app.apply(Action::Back); // first esc: close the drilldown
        assert_eq!(app.screen, Screen::Meter);
        app.apply(Action::Back); // second esc: back to the list
        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.list_selection(), 3, "cursor follows the segment");
    }

    #[test]
    fn an_in_progress_fight_jumps_straight_to_its_live_meter() {
        // Cut the fixture before its final ENCOUNTER_END: the last encounter
        // is open, and a fresh mtime says the game is still writing.
        let bytes = std::fs::read(crate::testkit::FIXTURE).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let cut = text.rfind("ENCOUNTER_END").unwrap();
        let idx = crate::index::scan(&mut &bytes[..cut]);
        assert!(idx.open.is_some(), "the fixture's last fight is open");
        let live = idx.live_offset as usize;

        let mut app = App::new();
        app.on_tail(TailEvent::Switched(PathBuf::from("/logs/a.txt")));
        app.on_tail(TailEvent::Index {
            index: idx.clone(),
            file_age_ms: Some(1_000),
        });
        assert_eq!(app.screen, Screen::Meter, "mid-fight: skip the list");
        assert!(app.following_live());

        let tail: Vec<String> = text[live..cut].lines().map(str::to_string).collect();
        app.on_tail(TailEvent::Lines(tail));
        assert!(app.is_live());
        assert_eq!(app.segment_name().as_deref(), Some("Verkath the Hollow"));
        assert!(!app.rows().is_empty(), "the live fight has data");

        // The same open segment in a stale file is just history-in-waiting.
        let mut stale = App::new();
        stale.on_tail(TailEvent::Switched(PathBuf::from("/logs/a.txt")));
        stale.on_tail(TailEvent::Index {
            index: idx,
            file_age_ms: Some(24 * 60 * 60 * 1000),
        });
        assert_eq!(stale.screen, Screen::List, "old file: nothing is live");
    }

    #[test]
    fn the_live_segment_appears_at_the_end_of_the_list() {
        let bytes = std::fs::read(crate::testkit::FIXTURE).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let cut = text.rfind("ENCOUNTER_END").unwrap();
        let idx = crate::index::scan(&mut &bytes[..cut]);
        let live = idx.live_offset as usize;

        let mut app = App::new();
        app.on_tail(TailEvent::Switched(PathBuf::from("/logs/a.txt")));
        app.on_tail(TailEvent::Index {
            index: idx,
            file_age_ms: None, // metadata unreadable: don't claim live
        });
        assert_eq!(app.screen, Screen::List);
        app.on_tail(TailEvent::Lines(
            text[live..cut].lines().map(str::to_string).collect(),
        ));

        let rows = app.list_rows();
        assert_eq!(rows.len(), 4);
        assert!(rows.last().unwrap().live, "the open fight is marked live");
        assert!(rows[..3].iter().all(|r| !r.live));
    }

    #[test]
    fn the_loaded_cache_is_bounded() {
        let mut app = indexed_app();
        app.apply(Action::Open);
        service_loads(&mut app);
        // Walk the whole history a few times; the cache must not grow past
        // its cap or lose the ability to serve the current selection.
        for _ in 0..3 {
            for _ in 0..app.segment_count() {
                app.apply(Action::OlderSegment);
                service_loads(&mut app);
            }
            for _ in 0..app.segment_count() {
                app.apply(Action::NewerSegment);
                service_loads(&mut app);
            }
        }
        assert!(app.segment().is_some());
        assert!(app.loaded.len() <= LOADED_CAP);
    }

    #[test]
    fn a_new_log_file_clears_the_index_and_cache() {
        let mut app = indexed_app();
        app.apply(Action::Open);
        service_loads(&mut app);
        assert!(app.segment_count() > 0);

        app.on_tail(TailEvent::Switched(PathBuf::from(
            "/logs/WoWCombatLog-02.txt",
        )));
        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.segment_count(), 0);
        assert!(app.list_rows().is_empty());
        assert!(app.load_request().is_none());
        assert!(app.segment().is_none());
    }
}
