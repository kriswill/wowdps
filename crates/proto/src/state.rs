//! Client-side application state: everything a frontend holds that is *not*
//! derived from the log — screen, selections, drilldown, follow-pin — plus
//! the last snapshot, cached so held-key navigation clamps locally and never
//! round-trips.
//!
//! The accessor surface deliberately matches the old `App`, so `ui.rs` and
//! `view.rs` render unchanged. The difference is all in `apply`/`on_msg`,
//! which return the `ClientMsg`s the frontend must send: state moves, and a
//! new cursor declaration follows it.

use wowdps_model::{Action, Drill, ListRow, Pane, Row, Screen, SegmentInfo, SegmentKind, View};

use crate::msg::{Breakdown, ClientMsg, Cursor, DaemonMsg, ListEntry, LoadError, SegmentRef};

/// The cached content of the last snapshot matching the current cursor.
struct Snap {
    view: View,
    info: SegmentInfo,
    rows: Vec<Row>,
    breakdown: Option<Breakdown>,
    segment_count: u32,
}

pub struct ClientState {
    pub screen: Screen,
    pub view: View,
    pub row_sel: usize,
    pub drill: Option<Drill>,
    /// Log file being followed, for the header.
    pub source: Option<String>,
    /// Daemon-side notice / error, for the footer.
    pub status: Option<String>,
    pub quit: bool,
    /// What the meter screen watches. `Live` doubles as the follow pin:
    /// watching Live *is* following the newest segment.
    cursor: SegmentRef,
    snapshot: Option<Snap>,
    /// The segment list as last pushed, oldest first — also the id table
    /// segment navigation resolves neighbors against. `SegmentOpened` keeps
    /// its tail fresh while the meter screen has the cursor.
    entries: Vec<ListEntry>,
    list_sel: usize,
    /// First `SegmentList` processed: the jump-to-live decision is made once.
    started: bool,
    /// Row cap requested from the daemon (overlay uses a small one).
    top_n: Option<u32>,
}

impl Default for ClientState {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientState {
    pub fn new() -> Self {
        Self {
            screen: Screen::List,
            view: View::Damage,
            row_sel: 0,
            drill: None,
            source: None,
            status: None,
            quit: false,
            cursor: SegmentRef::Live,
            snapshot: None,
            entries: Vec::new(),
            list_sel: 0,
            started: false,
            top_n: None,
        }
    }

    pub fn with_top_n(top_n: Option<u32>) -> Self {
        Self {
            top_n,
            ..Self::new()
        }
    }

    /// The first thing to send after the handshake.
    pub fn initial_request(&self) -> ClientMsg {
        self.watch_msg()
    }

    /// The Watch declaring what this state is currently rendering.
    fn watch_msg(&self) -> ClientMsg {
        match self.screen {
            Screen::List => ClientMsg::Watch(Cursor::List),
            Screen::Meter => ClientMsg::Watch(Cursor::Segment {
                segment: self.cursor,
                view: self.view,
                top_n: self.top_n,
                drill: self.drill.as_ref().map(|d| d.key.clone()),
            }),
        }
    }

    // ---- accessors (the old `App` surface) ----------------------------------

    pub fn rows(&self) -> Vec<Row> {
        match &self.snapshot {
            Some(s) if s.view == self.view => s.rows.clone(),
            _ => Vec::new(),
        }
    }

    pub fn breakdown(&self) -> (Vec<Row>, Vec<Row>) {
        if self.drill.is_none() {
            return (Vec::new(), Vec::new());
        }
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => (b.by_spell.clone(), b.by_target.clone()),
            _ => (Vec::new(), Vec::new()),
        }
    }

    pub fn list_rows(&self) -> Vec<ListRow> {
        self.entries.iter().map(|e| e.row.clone()).collect()
    }

    pub fn segment_count(&self) -> usize {
        match (&self.snapshot, self.screen) {
            (Some(s), Screen::Meter) => s.segment_count as usize,
            _ => self.entries.len(),
        }
    }

    pub fn segment_index(&self) -> usize {
        let count = self.segment_count();
        if count == 0 {
            return 0;
        }
        let pos = match self.cursor {
            SegmentRef::Live => count - 1,
            SegmentRef::Id(id) => self
                .entries
                .iter()
                .position(|e| e.id == id)
                .unwrap_or(count - 1),
        };
        pos.min(count - 1)
    }

    pub fn following_live(&self) -> bool {
        matches!(self.cursor, SegmentRef::Live)
    }

    pub fn is_live(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.info.live)
    }

    pub fn segment_name(&self) -> Option<String> {
        let s = self.snapshot.as_ref()?;
        (s.segment_count > 0).then(|| s.info.name.clone())
    }

    pub fn segment_success(&self) -> Option<bool> {
        self.snapshot.as_ref().and_then(|s| s.info.success)
    }

    /// R10: what the watched segment is — headers word success by kind
    /// (KILL/WIPE for encounters, TIMED/OVER for a keyed visit's overall).
    pub fn segment_kind(&self) -> Option<SegmentKind> {
        self.snapshot.as_ref().map(|s| s.info.kind)
    }

    /// R10: the instance visit the watched segment belongs to.
    pub fn segment_instance(&self) -> Option<u32> {
        self.snapshot.as_ref().and_then(|s| s.info.instance)
    }

    pub fn duration_ms(&self) -> i64 {
        self.snapshot.as_ref().map_or(0, |s| s.info.duration_ms)
    }

    pub fn list_selection(&self) -> usize {
        self.list_sel
    }

    /// The id table as last pushed, oldest first — position-aligned with
    /// `list_rows`. Frontends that group segments (the overlay's instance
    /// timeline) resolve clicks against these positions.
    pub fn entries(&self) -> &[ListEntry] {
        &self.entries
    }

    /// Jump the meter straight to a combined-list position, from any screen.
    /// The newest position pins to Live (following); anything else watches a
    /// stable id. Pointer-driven frontends use this for direct jumps that
    /// aren't a walk over Older/NewerSegment.
    pub fn goto_list_pos(&mut self, pos: usize) -> Vec<ClientMsg> {
        self.goto_pos(pos)
    }

    /// Re-pin the meter to Live. A no-op when already following, so callers
    /// can invoke it on every "combat started" signal without churn.
    pub fn pin_live(&mut self) -> Vec<ClientMsg> {
        if self.screen == Screen::Meter && self.following_live() {
            return Vec::new();
        }
        self.screen = Screen::Meter;
        self.cursor = SegmentRef::Live;
        self.row_sel = 0;
        self.drill = None;
        self.snapshot = None;
        vec![self.watch_msg()]
    }

    /// Point the list cursor at a row directly — pointer-driven frontends
    /// select by position, not by walking Up/Down.
    pub fn set_list_selection(&mut self, row: usize) {
        let count = self.entries.len();
        self.list_sel = if count == 0 { 0 } else { row.min(count - 1) };
    }

    // ---- daemon messages ----------------------------------------------------

    /// Digest one daemon message; the returned requests must be sent.
    pub fn on_msg(&mut self, msg: DaemonMsg) -> Vec<ClientMsg> {
        match msg {
            DaemonMsg::Snapshot {
                segment,
                id,
                view,
                info,
                rows,
                breakdown,
                segment_count,
                source,
                status,
                ..
            } => {
                if self.rotated(&source) {
                    return self.reset_for_new_source(source);
                }
                self.source = source;
                self.status = status;
                // Only the current cursor's snapshots count; a push that was
                // in flight when the cursor changed is simply stale.
                if self.screen != Screen::Meter || segment != self.cursor || view != self.view {
                    return Vec::new();
                }
                // Watching Live, the daemon tells us which id that actually
                // is — keep the id table's tail honest even off the list
                // screen.
                if let Some(id) = id
                    && !self.entries.iter().any(|e| e.id == id)
                {
                    self.entries.push(ListEntry {
                        id,
                        row: list_row_of(&info),
                    });
                }
                self.snapshot = Some(Snap {
                    view,
                    info,
                    rows,
                    breakdown,
                    segment_count,
                });
                self.clamp_selection();
                Vec::new()
            }
            DaemonMsg::SegmentList {
                entries,
                source,
                active,
                ..
            } => {
                if self.rotated(&source) {
                    return self.reset_for_new_source(source);
                }
                self.source = source;
                let first = !self.started;
                self.started = true;
                self.entries = entries;
                let count = self.entries.len();
                self.list_sel = if first || self.list_sel >= count {
                    count.saturating_sub(1)
                } else {
                    self.list_sel
                };
                // Arriving mid-fight skips the list: the meter is what you
                // want mid-pull. The daemon's `active` verdict replaces the
                // old mtime guess.
                if first && active && self.screen == Screen::List {
                    self.screen = Screen::Meter;
                    self.cursor = SegmentRef::Live;
                    return vec![self.watch_msg()];
                }
                Vec::new()
            }
            DaemonMsg::SegmentOpened { id } => {
                if !self.entries.iter().any(|e| e.id == id) {
                    self.entries.push(ListEntry {
                        id,
                        row: ListRow {
                            kind: wowdps_model::SegmentKind::Trash,
                            name: String::new(),
                            start_ms: 0,
                            success: None,
                            duration_ms: 0,
                            live: true,
                            instance: None,
                        },
                    });
                }
                // A fight starting *now* pulls a pinned list back to the
                // meter; backing out mid-fight sticks until the next pull.
                if self.screen == Screen::List && self.following_live() && self.started {
                    self.screen = Screen::Meter;
                    self.cursor = SegmentRef::Live;
                    self.row_sel = 0;
                    self.drill = None;
                    return vec![self.watch_msg()];
                }
                Vec::new()
            }
            DaemonMsg::LoadFailed { error, .. } => {
                self.status = Some(match error {
                    LoadError::NotFound => "segment not found".to_string(),
                    LoadError::Rotated => "segment gone: the log rotated".to_string(),
                    LoadError::Io(e) => e,
                });
                Vec::new()
            }
            DaemonMsg::Fatal(msg) => {
                self.status = Some(msg);
                Vec::new()
            }
            DaemonMsg::HelloAck { .. } | DaemonMsg::Status { .. } | DaemonMsg::SetVisible(_) => {
                Vec::new()
            }
        }
    }

    fn rotated(&self, source: &Option<String>) -> bool {
        matches!((&self.source, source), (Some(old), Some(new)) if old != new)
    }

    /// A different log file is a different session: start over on its list.
    fn reset_for_new_source(&mut self, source: Option<String>) -> Vec<ClientMsg> {
        *self = Self {
            source,
            top_n: self.top_n,
            quit: self.quit,
            ..Self::new()
        };
        vec![self.watch_msg()]
    }

    // ---- actions ------------------------------------------------------------

    /// Apply a key action; the returned requests must be sent.
    pub fn apply(&mut self, action: Action) -> Vec<ClientMsg> {
        match self.screen {
            Screen::List => self.apply_list(action),
            Screen::Meter => self.apply_meter(action),
        }
    }

    fn apply_list(&mut self, action: Action) -> Vec<ClientMsg> {
        let count = self.entries.len();
        match action {
            Action::Quit => self.quit = true,
            Action::SetView(view) => self.view = view,
            Action::Up => self.list_sel = self.list_sel.saturating_sub(1),
            Action::Down => {
                if count > 0 {
                    self.list_sel = (self.list_sel + 1).min(count - 1);
                }
            }
            Action::Open if count > 0 => {
                return self.goto_pos(self.list_sel.min(count - 1));
            }
            _ => {}
        }
        Vec::new()
    }

    fn apply_meter(&mut self, action: Action) -> Vec<ClientMsg> {
        match action {
            Action::Quit => {
                self.quit = true;
                Vec::new()
            }
            Action::SetView(view) => {
                self.view = view;
                // The drilldown follows the player across views, like always.
                vec![self.watch_msg()]
            }
            Action::OlderSegment => {
                let pos = self.segment_index();
                if pos == 0 {
                    return Vec::new();
                }
                self.goto_pos(pos - 1)
            }
            Action::NewerSegment => {
                let pos = self.segment_index();
                if pos + 1 >= self.segment_count() {
                    return Vec::new();
                }
                self.goto_pos(pos + 1)
            }
            Action::Up => {
                self.move_selection(-1);
                Vec::new()
            }
            Action::Down => {
                self.move_selection(1);
                Vec::new()
            }
            Action::Open => self.open_drilldown(),
            Action::Back => {
                if self.drill.is_some() {
                    self.drill = None;
                    vec![self.watch_msg()]
                } else {
                    // Leave the meter for the list, cursor on this segment.
                    self.list_sel = self
                        .segment_index()
                        .min(self.entries.len().saturating_sub(1));
                    self.screen = Screen::List;
                    vec![self.watch_msg()]
                }
            }
            Action::SwapPane => {
                if let Some(drill) = self.drill.as_mut() {
                    drill.pane = match drill.pane {
                        Pane::Spell => Pane::Target,
                        Pane::Target => Pane::Spell,
                    };
                }
                Vec::new()
            }
        }
    }

    /// Jump the meter to a combined-list position. The newest position pins
    /// to Live (following); anything else watches a stable id.
    fn goto_pos(&mut self, pos: usize) -> Vec<ClientMsg> {
        let count = self.entries.len();
        if count == 0 {
            return Vec::new();
        }
        // "Newest" by the same metric `segment_index` uses — the daemon's
        // count when a snapshot is in hand. When the id table lags that
        // count, a non-newest position it cannot resolve is a no-op:
        // staying put beats silently re-pinning Live.
        let newest = count.max(self.segment_count()) - 1;
        self.cursor = if pos >= newest {
            SegmentRef::Live
        } else if let Some(entry) = self.entries.get(pos) {
            SegmentRef::Id(entry.id)
        } else {
            return Vec::new();
        };
        self.screen = Screen::Meter;
        self.row_sel = 0;
        self.drill = None;
        self.snapshot = None;
        vec![self.watch_msg()]
    }

    fn open_drilldown(&mut self) -> Vec<ClientMsg> {
        if self.drill.is_some() {
            return Vec::new();
        }
        let rows = self.rows();
        let Some(row) = rows.get(self.row_sel) else {
            return Vec::new();
        };
        self.drill = Some(Drill {
            key: row.key.clone(),
            label: row.label.clone(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
        });
        vec![self.watch_msg()]
    }

    /// Held-key repeat clamps against the cached snapshot — never a request.
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

    /// Rows re-sort and disappear between snapshots; never point past the end.
    fn clamp_selection(&mut self) {
        let len = self.rows().len();
        self.row_sel = if len == 0 {
            0
        } else {
            self.row_sel.min(len - 1)
        };
        let (by_spell, by_target) = self.breakdown();
        if let Some(drill) = self.drill.as_mut() {
            drill.spell_sel = clamp_to(drill.spell_sel, by_spell.len());
            drill.target_sel = clamp_to(drill.target_sel, by_target.len());
        }
    }
}

fn list_row_of(info: &SegmentInfo) -> ListRow {
    ListRow {
        kind: info.kind,
        name: info.name.clone(),
        start_ms: info.start_ms,
        success: info.success,
        duration_ms: info.duration_ms,
        live: info.live,
        instance: info.instance,
    }
}

fn clamp_to(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}
