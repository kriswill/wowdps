//! Client-side application state: everything a frontend holds that is *not*
//! derived from the log — screen, selections, drilldown, follow-pin — plus
//! the last snapshot, cached so held-key navigation clamps locally and never
//! round-trips.
//!
//! The accessor surface deliberately matches the old `App`, so `ui.rs` and
//! `view.rs` render unchanged. The difference is all in `apply`/`on_msg`,
//! which return the `ClientMsg`s the frontend must send: state moves, and a
//! new cursor declaration follows it.

use wowdps_model::{
    Action, Drill, GraphMode, ListRow, Pane, Row, Screen, SegmentInfo, SegmentKind, Timeline, View,
};

use crate::msg::{
    Breakdown, ClientMsg, CompareSide, Cursor, DaemonMsg, ListEntry, LoadError, SegmentRef,
};

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
    /// R12: the players picked for comparison, in pick order — at most two.
    /// Kept as (guid, label) so a pick survives the player dropping off the
    /// snapshot entirely (a mage who stops casting still has a name).
    compare: Vec<(String, String)>,
    /// R12: the last comparison pushed for the current pair.
    compare_snap: Option<(CompareSide, CompareSide)>,
    /// R12/v12: the requested comparison window (ms from segment start) —
    /// what the next Watch asks for. `None` is the whole fight.
    compare_range: Option<(u32, u32)>,
    /// R12/v12: the window the daemon's last answer was computed over (the
    /// snapshot's echo). Renderers gate the zoomed view on this, never on
    /// `compare_range`, so tables and graph always agree.
    compare_snap_range: Option<(u32, u32)>,
    /// R12: which curve the comparison graphs draw. Purely local — the
    /// daemon always sends the buckets and lets the client shape them.
    graph: GraphMode,
    /// v14: the drilldown graph's zoom window (ms from segment start).
    /// Purely local — the drill timeline arrives whole, so zooming is the
    /// client's own slice and never round-trips. Cleared with the drill.
    drill_range: Option<(u32, u32)>,
    /// v18: the comparison's ability drill — one (by-spell key, label)
    /// applied to BOTH sides. Cleared one level ahead of the pair.
    compare_spell: Option<(String, String)>,
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
            compare: Vec::new(),
            compare_snap: None,
            compare_range: None,
            compare_snap_range: None,
            graph: GraphMode::default(),
            drill_range: None,
            compare_spell: None,
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

    /// The segment the meter screen watches right now — what a one-shot
    /// query (v19 `GetLoadout`) should name to ask about "this fight".
    pub fn watched_segment(&self) -> SegmentRef {
        self.cursor
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
                spell: self
                    .drill
                    .as_ref()
                    .and_then(|d| d.spell.as_ref().map(|(k, _)| k.clone())),
            }),
            // R12. `Screen::Compare` is only ever entered with both picks in
            // hand, so the pair is always there to name.
            Screen::Compare => match (self.compare.first(), self.compare.get(1)) {
                (Some((a, _)), Some((b, _))) => ClientMsg::Watch(Cursor::Compare {
                    segment: self.cursor,
                    a: a.clone(),
                    b: b.clone(),
                    range: self.compare_range,
                    spell: self.compare_spell.as_ref().map(|(k, _)| k.clone()),
                }),
                _ => ClientMsg::Watch(Cursor::Segment {
                    segment: self.cursor,
                    view: self.view,
                    top_n: self.top_n,
                    drill: None,
                    spell: None,
                }),
            },
        }
    }

    // ---- R12: comparison ----------------------------------------------------

    /// The players picked for comparison, in pick order.
    pub fn compare_picks(&self) -> &[(String, String)] {
        &self.compare
    }

    /// Which side of the pair a meter row is, if any — what a frontend uses
    /// to badge the class icon it drew next to the name.
    pub fn compare_slot(&self, key: &str) -> Option<usize> {
        self.compare.iter().position(|(g, _)| g == key)
    }

    /// The comparison as last pushed. `None` until both players are picked
    /// and the daemon has answered, which is exactly when a frontend should
    /// start drawing one.
    pub fn compare_sides(&self) -> Option<(&CompareSide, &CompareSide)> {
        let (a, b) = self.compare_snap.as_ref()?;
        Some((a, b))
    }

    pub fn graph_mode(&self) -> GraphMode {
        self.graph
    }

    /// R12/v12: the window the current `compare_snap` answers — the daemon's
    /// echo, so it can lag `compare_range` by a round trip.
    pub fn compare_shown_range(&self) -> Option<(u32, u32)> {
        self.compare_snap_range
    }

    /// v18: the comparison's open ability drill, as (key, label).
    pub fn compare_spell(&self) -> Option<&(String, String)> {
        self.compare_spell.as_ref()
    }

    /// v18: drill BOTH comparison sides into one ability — the clicked
    /// side's by-spell key, applied to the pair (same-class pairs share
    /// their kit; a side without the spell just shows no focus curve).
    pub fn drill_compare_spell(&mut self, key: &str, label: &str) -> Vec<ClientMsg> {
        if self.screen != Screen::Compare || self.compare.len() != 2 {
            return Vec::new();
        }
        self.compare_spell = Some((key.to_string(), label.to_string()));
        vec![self.watch_msg()]
    }

    /// R12/v12: window the comparison to `lo..hi` ms from the segment start,
    /// or `None` for the whole fight. Ignored outside an open comparison. A
    /// degenerate window (hi <= lo) clears instead — the drag that produced
    /// it selected nothing.
    pub fn set_compare_range(&mut self, range: Option<(u32, u32)>) -> Vec<ClientMsg> {
        if self.screen != Screen::Compare || self.compare.len() != 2 {
            return Vec::new();
        }
        let range = range.filter(|(lo, hi)| hi > lo);
        if range == self.compare_range {
            return Vec::new();
        }
        self.compare_range = range;
        vec![self.watch_msg()]
    }

    /// Add or remove a player from the pair. A third pick replaces the older
    /// of the two, so clicking around a raid frame keeps working without a
    /// clear step. The comparison screen opens on the second pick and closes
    /// as soon as the pair is broken.
    pub fn toggle_compare(&mut self, key: &str, label: &str) -> Vec<ClientMsg> {
        match self.compare.iter().position(|(g, _)| g == key) {
            Some(i) => {
                self.compare.remove(i);
            }
            None => {
                if self.compare.len() == 2 {
                    self.compare.remove(0);
                }
                self.compare.push((key.to_string(), label.to_string()));
            }
        }
        self.compare_snap = None;
        self.compare_range = None;
        self.compare_snap_range = None;
        self.compare_spell = None;
        let ready = self.compare.len() == 2;
        self.screen = if ready {
            Screen::Compare
        } else {
            Screen::Meter
        };
        vec![self.watch_msg()]
    }

    /// Drop the pair and return to the meter.
    pub fn clear_compare(&mut self) -> Vec<ClientMsg> {
        // v18: back out one level at a time — the ability drill first, the
        // pair only once no spell is open. Pointer parity comes free: the
        // frontends' right-click/Esc both land here.
        if self.screen == Screen::Compare && self.compare_spell.is_some() {
            self.compare_spell = None;
            return vec![self.watch_msg()];
        }
        if self.compare.is_empty() && self.screen != Screen::Compare {
            return Vec::new();
        }
        self.compare.clear();
        self.compare_snap = None;
        self.compare_range = None;
        self.compare_snap_range = None;
        self.screen = Screen::Meter;
        vec![self.watch_msg()]
    }

    /// Swap the graph between rolling DPS and cumulative damage. Local only:
    /// both curves come out of the same buckets already in hand.
    pub fn toggle_graph(&mut self) {
        self.graph = self.graph.toggled();
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

    /// v14: the drilled player's damage timeline, when the snapshot carries
    /// one (Damage view only). Same R12 grid the comparison draws.
    pub fn drill_timeline(&self) -> Option<&Timeline> {
        self.drill.as_ref()?;
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => b.timeline.as_ref(),
            _ => None,
        }
    }

    /// v14: the drill graph's zoom window. Local-only — the timeline is
    /// whole, so the renderer slices it itself.
    pub fn drill_range(&self) -> Option<(u32, u32)> {
        self.drill_range
    }

    /// v16: the open ability drill, as (by-spell key, display label).
    pub fn drill_spell(&self) -> Option<&(String, String)> {
        self.drill.as_ref()?.spell.as_ref()
    }

    /// v16: the drilled ability's own curve, when the snapshot carries one
    /// (Damage view only — like the player timeline it draws over).
    pub fn spell_timeline(&self) -> Option<&Timeline> {
        self.drill_spell()?;
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => b.spell_timeline.as_ref(),
            _ => None,
        }
    }

    /// v16: the drilled ability's stat row — its own by-spell breakdown row,
    /// which already carries total, hits, crits, extra, school and icon id.
    pub fn drill_spell_row(&self) -> Option<Row> {
        let key = self.drill_spell()?.0.clone();
        let (by_spell, _) = self.breakdown();
        by_spell.into_iter().find(|r| r.key == key)
    }

    /// v17: who the drilled ability landed on — sorted desc, pct of the
    /// spell's own total. Empty until the drilled snapshot arrives.
    pub fn spell_target_rows(&self) -> Vec<Row> {
        if self.drill_spell().is_none() {
            return Vec::new();
        }
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => b.spell_targets.clone().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// v14: the Σ graph's encounter lane — `[lo, hi)` ms spans, relative to
    /// the watched Overall's start, where its Encounter members ran. Empty
    /// on anything that is not an Overall, so renderers can gate on it.
    /// Computed from the segment list the client already holds: a member's
    /// `start_ms` shares the Overall's clock (the visit's wall clock, R12).
    pub fn encounter_spans(&self) -> Vec<(u32, u32)> {
        let Some(Snap { info, .. }) = &self.snapshot else {
            return Vec::new();
        };
        if info.kind != SegmentKind::Overall {
            return Vec::new();
        }
        let Some(ord) = info.instance else {
            return Vec::new();
        };
        self.entries
            .iter()
            .map(|e| &e.row)
            .filter(|r| r.kind == SegmentKind::Encounter && r.instance == Some(ord))
            .filter_map(|r| {
                let lo = (r.start_ms - info.start_ms).max(0);
                let hi = lo + r.duration_ms.max(0);
                let (lo, hi) = (u32::try_from(lo).ok()?, u32::try_from(hi).ok()?);
                (hi > lo).then_some((lo, hi))
            })
            .collect()
    }

    pub fn set_drill_range(&mut self, range: Option<(u32, u32)>) {
        // A degenerate selection means zoom out, like the comparison's.
        self.drill_range = range.filter(|(lo, hi)| lo < hi);
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

    /// R10: the watched keyed Overall's (par, +2, +3) timers.
    pub fn segment_pars_ms(&self) -> Option<(i64, i64, i64)> {
        self.snapshot.as_ref().and_then(|s| s.info.pars_ms)
    }

    /// R10: what the watched segment is — headers word success by kind
    /// (KILL/WIPE for encounters, TIMED/OVER for a keyed visit's overall).
    pub fn segment_kind(&self) -> Option<SegmentKind> {
        self.snapshot.as_ref().map(|s| s.info.kind)
    }

    /// R13: the watched segment is an arena match — headers word `success`
    /// as WIN/LOSS instead of KILL/WIPE.
    pub fn segment_arena(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.info.arena)
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
        let comparing = self.screen == Screen::Compare && self.compare.len() == 2;
        if (self.screen == Screen::Meter || comparing) && self.following_live() {
            return Vec::new();
        }
        // R12: returning to live keeps an open comparison, like any other
        // segment move — the pair follows onto the live fight.
        if comparing {
            self.compare_snap = None;
            self.compare_range = None;
            self.compare_snap_range = None;
        } else {
            self.screen = Screen::Meter;
        }
        self.cursor = SegmentRef::Live;
        self.row_sel = 0;
        self.drill = None;
        self.drill_range = None;
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
            // R12. Like `Snapshot`: rotation resets, and a push that raced a
            // cursor change is stale and dropped.
            DaemonMsg::CompareSnapshot {
                segment,
                info,
                a,
                b,
                range,
                source,
                status,
                ..
            } => {
                if self.rotated(&source) {
                    return self.reset_for_new_source(source);
                }
                self.source = source;
                self.status = status;
                if self.screen != Screen::Compare || segment != self.cursor {
                    return Vec::new();
                }
                let pair_matches = matches!(
                    (self.compare.first(), self.compare.get(1)),
                    (Some((x, _)), Some((y, _))) if *x == a.guid && *y == b.guid
                );
                if !pair_matches {
                    return Vec::new();
                }
                // The header (duration, name, live flag) comes along so the
                // comparison screen doesn't need a meter snapshot underneath.
                if let Some(s) = self.snapshot.as_mut() {
                    s.info = info;
                } else {
                    self.snapshot = Some(Snap {
                        view: self.view,
                        info,
                        rows: Vec::new(),
                        breakdown: None,
                        segment_count: self.entries.len() as u32,
                    });
                }
                self.compare_snap = Some((*a, *b));
                self.compare_snap_range = range;
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
                            arena: false,
                            encounter: None,
                            pars_ms: None,
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
                    self.drill_range = None;
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
            // Loadout is a one-shot reply the requesting frontend consumes
            // itself (the GUI intercepts it before this machine sees it) —
            // benign everywhere else, like Status.
            // v20: the history replies likewise — a history screen is
            // window-local (item 2) and reads them before this machine.
            DaemonMsg::HelloAck { .. }
            | DaemonMsg::Status { .. }
            | DaemonMsg::SetVisible(_)
            | DaemonMsg::Loadout { .. }
            | DaemonMsg::History { .. }
            | DaemonMsg::Fight { .. }
            | DaemonMsg::HistoryChanged { .. } => Vec::new(),
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
            Screen::Compare => self.apply_compare(action),
        }
    }

    /// R12. The comparison has no row selection, but segment navigation
    /// still works — the pair sticks and follows onto the neighbor.
    fn apply_compare(&mut self, action: Action) -> Vec<ClientMsg> {
        match action {
            Action::Quit => {
                self.quit = true;
                Vec::new()
            }
            Action::ToggleGraph => {
                self.toggle_graph();
                Vec::new()
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
            Action::Back | Action::PickCompare => self.clear_compare(),
            _ => Vec::new(),
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
                // The drilldown follows the player across views, like always
                // — but not the ABILITY drill: by-spell keys are view-local
                // ("Flash Heal" is not a damage row), so it closes (v16).
                if let Some(d) = self.drill.as_mut() {
                    d.spell = None;
                    self.drill_range = None;
                }
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
            // R12: pick the highlighted player. Nothing opens until the
            // second pick lands, so a lone pick just sits there badged.
            Action::PickCompare => {
                let rows = self.rows();
                match rows.get(self.row_sel) {
                    Some(r) => {
                        let (key, label) = (r.key.clone(), r.label.clone());
                        self.toggle_compare(&key, &label)
                    }
                    None => Vec::new(),
                }
            }
            Action::ToggleGraph => {
                self.toggle_graph();
                Vec::new()
            }
            Action::Open => {
                if self.drill.is_some() {
                    // v16: Enter inside a drilldown descends into the
                    // selected ability.
                    self.open_spell_drill()
                } else {
                    self.open_drilldown()
                }
            }
            Action::Back => {
                // v16: back out one level at a time — ability, drill, list.
                if let Some(d) = self.drill.as_mut()
                    && d.spell.is_some()
                {
                    d.spell = None;
                    self.drill_range = None;
                    vec![self.watch_msg()]
                } else if self.drill.is_some() {
                    self.drill = None;
                    self.drill_range = None;
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
                if let Some(drill) = self.drill.as_mut()
                    && drill.spell.is_none()
                {
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
        // R12: segment navigation never breaks an open comparison — the
        // pair sticks and the new segment's sides are requested for it. The
        // stale sides are dropped rather than shown under the new header.
        if self.screen == Screen::Compare && self.compare.len() == 2 {
            self.compare_snap = None;
            self.compare_range = None;
            self.compare_snap_range = None;
        } else {
            self.screen = Screen::Meter;
        }
        self.row_sel = 0;
        self.drill = None;
        self.drill_range = None;
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
        self.drill_range = None;
        self.drill = Some(Drill {
            key: row.key.clone(),
            label: row.label.clone(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
            spell: None,
        });
        vec![self.watch_msg()]
    }

    /// v16: the second drill level — open the by-spell pane's selected row
    /// as an ability drill. Damage/Healing only (the ability view is
    /// graph-centric); no-ops when one is already open.
    fn open_spell_drill(&mut self) -> Vec<ClientMsg> {
        if !matches!(self.view, View::Damage | View::Healing) {
            return Vec::new();
        }
        let (by_spell, _) = self.breakdown();
        let Some(drill) = self.drill.as_mut() else {
            return Vec::new();
        };
        if drill.spell.is_some() || drill.pane != Pane::Spell {
            return Vec::new();
        }
        let Some(row) = by_spell.get(drill.spell_sel) else {
            return Vec::new();
        };
        drill.spell = Some((row.key.clone(), row.label.clone()));
        self.drill_range = None;
        vec![self.watch_msg()]
    }

    /// Held-key repeat clamps against the cached snapshot — never a request.
    fn move_selection(&mut self, delta: isize) {
        // v16: the ability view has no list to move through.
        if self.drill_spell().is_some() {
            return;
        }
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
        pars_ms: info.pars_ms,
        arena: info.arena,
        encounter: info.encounter,
    }
}

fn clamp_to(index: usize, len: usize) -> usize {
    if len == 0 { 0 } else { index.min(len - 1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_model::SegmentId;

    fn row(kind: SegmentKind, start_ms: i64, duration_ms: i64, instance: Option<u32>) -> ListRow {
        ListRow {
            kind,
            name: String::new(),
            start_ms,
            success: None,
            duration_ms,
            live: false,
            instance,
            pars_ms: None,
            arena: false,
            encounter: None,
        }
    }

    /// v14: the Σ graph's encounter lane comes from the list the client
    /// already holds — Encounter members of the watched visit only, spans
    /// rebased onto the Overall's start and degenerate ones dropped.
    #[test]
    fn encounter_spans_pick_the_watched_visits_bosses() {
        let mut st = ClientState::new();
        st.screen = Screen::Meter;
        st.on_msg(DaemonMsg::SegmentList {
            seq: 1,
            entries: [
                row(SegmentKind::Encounter, 15_000, 60_000, Some(0)),
                row(SegmentKind::Trash, 80_000, 20_000, Some(0)),
                row(SegmentKind::Encounter, 200_000, 30_000, Some(1)),
                row(SegmentKind::Encounter, 300_000, 30_000, None),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, row)| ListEntry {
                id: SegmentId(i as u64),
                row,
            })
            .collect(),
            source: None,
            active: false,
            log_id: None,
        });
        st.screen = Screen::Meter;
        st.on_msg(DaemonMsg::Snapshot {
            seq: 2,
            segment: SegmentRef::Live,
            id: None,
            view: View::Damage,
            info: SegmentInfo {
                kind: SegmentKind::Overall,
                name: String::new(),
                start_ms: 5_000,
                duration_ms: 100_000,
                success: None,
                live: true,
                instance: Some(0),
                pars_ms: None,
                arena: false,
                encounter: None,
            },
            rows: Vec::new(),
            total_rows: 0,
            breakdown: None,
            segment_count: 4,
            source: None,
            status: None,
        });
        assert_eq!(st.encounter_spans(), vec![(10_000, 70_000)]);
    }

    /// Anything that is not an Overall draws no lane.
    #[test]
    fn encounter_spans_are_empty_off_the_overall() {
        let st = ClientState::new();
        assert!(st.encounter_spans().is_empty());
    }

    fn plain_row(key: &str) -> Row {
        Row {
            key: key.to_string(),
            label: key.to_string(),
            amount: 100,
            ..Row::default()
        }
    }

    fn snap(breakdown: Option<Breakdown>) -> DaemonMsg {
        DaemonMsg::Snapshot {
            seq: 1,
            segment: SegmentRef::Live,
            id: None,
            view: View::Damage,
            info: SegmentInfo {
                kind: SegmentKind::Encounter,
                name: String::new(),
                start_ms: 0,
                duration_ms: 1,
                success: None,
                live: true,
                instance: None,
                pars_ms: None,
                arena: false,
                encounter: None,
            },
            rows: vec![plain_row("Player-1")],
            total_rows: 1,
            breakdown,
            segment_count: 1,
            source: None,
            status: None,
        }
    }

    /// v16: Enter descends meter → drill → ability, the Watch names the
    /// spell, and Back pops exactly one level at a time.
    #[test]
    fn v16_ability_drill_descends_and_backs_out_one_level_at_a_time() {
        let mut st = ClientState::new();
        st.screen = Screen::Meter;
        st.on_msg(snap(None));
        st.apply(Action::Open);
        assert!(st.drill.is_some());
        st.on_msg(snap(Some(Breakdown {
            by_spell: vec![plain_row("Fireball")],
            by_target: vec![],
            timeline: None,
            spell_timeline: None,
            spell_targets: None,
            mitigation: None,
        })));
        let msgs = st.apply(Action::Open);
        assert_eq!(
            st.drill_spell().map(|(k, _)| k.as_str()),
            Some("Fireball"),
            "Enter in the spell pane opens the ability"
        );
        assert!(
            matches!(
                &msgs[..],
                [ClientMsg::Watch(Cursor::Segment { spell: Some(s), .. })] if s == "Fireball"
            ),
            "the Watch names the drilled spell"
        );
        st.apply(Action::Back);
        assert!(st.drill_spell().is_none(), "Back pops the ability…");
        assert!(st.drill.is_some(), "…but keeps the player drill");
        st.apply(Action::Back);
        assert!(st.drill.is_none(), "Back again closes the drill");
    }

    /// v18: the comparison's ability drill names the spell on the Watch,
    /// and backing out pops the spell BEFORE the pair.
    #[test]
    fn v18_compare_ability_drill_backs_out_before_the_pair() {
        let mut st = ClientState::new();
        st.screen = Screen::Meter;
        st.on_msg(snap(None));
        st.toggle_compare("A", "Ana");
        st.toggle_compare("B", "Bo");
        assert_eq!(st.screen, Screen::Compare);
        let msgs = st.drill_compare_spell("Fireball", "Fireball");
        assert!(
            matches!(
                &msgs[..],
                [ClientMsg::Watch(Cursor::Compare { spell: Some(s), .. })] if s == "Fireball"
            ),
            "the Watch names the drilled spell"
        );
        st.clear_compare();
        assert!(st.compare_spell().is_none(), "Esc pops the ability…");
        assert_eq!(st.screen, Screen::Compare, "…the pair survives");
        st.clear_compare();
        assert_eq!(st.screen, Screen::Meter, "the second Esc clears the pair");
    }
}
