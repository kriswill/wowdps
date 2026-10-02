//! Client-side application state: everything a frontend holds that is *not*
//! derived from the log — screen, selections, drilldown, follow-pin — plus
//! the last snapshot, cached so held-key navigation clamps locally and never
//! round-trips.
//!
//! The accessor surface deliberately matches the old `App`, so `ui.rs` and
//! `view.rs` render unchanged. The difference is all in `apply`/`on_msg`,
//! which return the `ClientMsg`s the frontend must send: state moves, and a
//! new cursor declaration follows it.

use wowdps_model::AbilitySeries;
use wowdps_model::{
    Action, Drill, Encounter, GraphMode, ListRow, Mitigation, Pane, RaidTimeline, Row, Screen,
    SegmentInfo, SegmentKind, SpellTree, StackBase, StackCell, StackingDebuff, Timeline, View,
};

use crate::msg::{
    Breakdown, ClientMsg, CompareSide, Cursor, DaemonMsg, DeathWindow, ListEntry, LoadError,
    SegmentRef,
};

/// The cached content of the last snapshot matching the current cursor.
struct Snap {
    view: View,
    info: SegmentInfo,
    rows: Vec<Row>,
    breakdown: Option<Breakdown>,
    segment_count: u32,
    /// v35 (R25): the segment's raid timeline, as the snapshot carried it.
    raid: Option<RaidTimeline>,
}

pub struct ClientState {
    pub screen: Screen,
    pub view: View,
    pub row_sel: usize,
    pub drill: Option<Drill>,
    /// v28 (R9): which of the drilled player's death windows the Deaths
    /// drill describes, by the index `Breakdown::deaths` carries. `None`
    /// is the last death. Reset whenever the drill or the view changes.
    death: Option<u32>,
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
    /// v29: the view the last comparison answers, echoed from the snapshot.
    /// Renderers word the tables and the curve from THIS, so a snapshot in
    /// flight when the view changed cannot label damage rows as taken.
    compare_snap_view: Option<View>,
    /// R12: which curve the comparison graphs draw. Purely local — the
    /// daemon always sends the buckets and lets the client shape them.
    graph: GraphMode,
    /// v14: the drilldown graph's zoom window (ms from segment start).
    /// The drill timeline arrives whole, so the curve's zoom is the
    /// client's own slice; where the view windows its drill
    /// (`View::windows_drill`: the enemies' attackers, v33, and Damage's
    /// and Healing's abilities, v38) the window also rides the Watch and
    /// scopes the rows. Cleared with the drill.
    drill_range: Option<(u32, u32)>,
    /// v38: the daemon behind this state answers a drill's window. A live
    /// one does; a stored pull's synthetic answers do not (the store keeps
    /// no per-second abilities), so its state never sends one and keeps
    /// every zoom the client's own.
    drill_windows: bool,
    /// v18: the comparison's ability drill — one (by-spell key, label)
    /// applied to BOTH sides. Cleared one level ahead of the pair.
    compare_spell: Option<(String, String)>,
    /// Opt-in master–detail (the window's inspector; the TUI never sets
    /// it): the drill FOLLOWS the meter selection. Every move re-watches
    /// the segment with the selected row's key as the drill, so each
    /// snapshot carries the rows and the breakdown and the screen stays
    /// the meter. `PickCompare` pins a player instead of picking one of
    /// two, and the selection is the pair's other half.
    follow: bool,
    /// While following: the keys walk the drill's own panes — Enter put
    /// them there, as it opened a drill before — instead of the meter's
    /// rows. A narrow window draws this as its pushed inspector. Back
    /// gives the keys back to the meter.
    inspecting: bool,
    /// How many snapshots the meter has taken in: a renderer that holds
    /// something derived from the breakdown in hand knows, by this moving,
    /// that the breakdown under it did. Additive: nothing here reads it.
    snapshot_gen: u64,
    /// v20: the tailed log's identity, as the last `SegmentList` named it —
    /// with a row's `start_ms` it is that row's history-store fight id.
    /// Additive: nothing here reads it.
    log_id: Option<u64>,
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
            death: None,
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
            compare_snap_view: None,
            graph: GraphMode::default(),
            drill_range: None,
            drill_windows: true,
            compare_spell: None,
            follow: false,
            inspecting: false,
            snapshot_gen: 0,
            log_id: None,
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
                death: self.death,
                spell: self
                    .drill
                    .as_ref()
                    .and_then(|d| d.spell.as_ref().map(|(k, _)| k.clone())),
                // v33 (R24): the zoom window scopes the enemy drill's rows;
                // v38: Damage's and Healing's too. Elsewhere the zoom is the
                // client's own.
                range: self.drill_windowed().then_some(self.drill_range).flatten(),
            }),
            // R12. `Screen::Compare` is only ever entered with both picks in
            // hand, so the pair is always there to name.
            Screen::Compare => match (self.compare.first(), self.compare.get(1)) {
                (Some((a, _)), Some((b, _))) => ClientMsg::Watch(Cursor::Compare {
                    segment: self.cursor,
                    a: a.clone(),
                    b: b.clone(),
                    // v29: a comparison is about the view it was opened
                    // from — pick two tanks on Taken and it compares what
                    // hit them, not what they hit back with.
                    view: self.view,
                    range: self.compare_range,
                    spell: self.compare_spell.as_ref().map(|(k, _)| k.clone()),
                }),
                _ => ClientMsg::Watch(Cursor::Segment {
                    segment: self.cursor,
                    view: self.view,
                    top_n: self.top_n,
                    drill: None,
                    death: None,
                    spell: None,
                    range: None,
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

    /// v29: the view the current comparison answers — the snapshot's own
    /// echo, which can lag `view` by a round trip. Falls back to `view`
    /// before the first answer, so the screen never words itself blank.
    pub fn compare_view(&self) -> View {
        self.compare_snap_view.unwrap_or(self.view)
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
        self.forget_compare_snap();
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
        self.forget_compare_snap();
        self.screen = Screen::Meter;
        vec![self.watch_msg()]
    }

    /// Swap the graph between rolling DPS and cumulative damage. Local only:
    /// both curves come out of the same buckets already in hand.
    pub fn toggle_graph(&mut self) {
        self.graph = self.graph.toggled();
    }

    // ---- follow-selection: the window's inspector ---------------------------

    /// Turn follow-selection on or off (see the `follow` field). Off is
    /// what every client gets and what the TUI keeps: a drill is a screen
    /// the reader enters and leaves. On, the drill is the selected row's
    /// for as long as the meter is up — turning it on over a meter already
    /// showing names the selection's drill at once.
    pub fn set_follow(&mut self, on: bool) -> Vec<ClientMsg> {
        if self.follow == on {
            return Vec::new();
        }
        self.follow = on;
        self.inspecting = false;
        if on {
            // A drill already open keeps its player: the selection moves to
            // them, rather than the drill to whatever row it was on.
            self.follow_snapshot()
        } else {
            Vec::new()
        }
    }

    /// Is follow-selection on?
    pub fn follows_selection(&self) -> bool {
        self.follow
    }

    /// Following, the keys walk the drill's panes rather than the meter's
    /// rows: a narrow window shows the inspector on its own.
    pub fn inspecting(&self) -> bool {
        self.follow && self.inspecting
    }

    /// Give the keys to the drill's panes — what Enter does while
    /// following, and a narrow window's click on a row. Nothing to inspect
    /// before the selection has a drill (or a pair).
    pub fn inspect(&mut self) {
        let subject = match self.screen {
            Screen::Meter => self.drill.is_some(),
            Screen::Compare => true,
            Screen::List => false,
        };
        if self.follow && subject {
            self.inspecting = true;
        }
    }

    /// Give the keys back to the meter's rows.
    pub fn uninspect(&mut self) {
        self.inspecting = false;
    }

    /// Select the meter row `row` — a click, or a step the window worked
    /// out over what it draws (a filtered, a sorted list). Following, the
    /// drill (or the pair) moves with it and the Watch that says so comes
    /// back; the keys return to the meter, since the reader just acted on
    /// it. Off, it is the plain assignment it always was.
    pub fn select_row(&mut self, row: usize) -> Vec<ClientMsg> {
        let len = self.rows().len();
        self.row_sel = if len == 0 { 0 } else { row.min(len - 1) };
        if !self.follow {
            return Vec::new();
        }
        self.inspecting = false;
        self.follow_sync()
    }

    /// Following: select the player `key` (`label` their name) on the
    /// meter — the window's command palette, which names a player rather
    /// than a row. Their row when the chart in hand has one; else, on the
    /// meter, the drill put on them and the Watch that says so, for the
    /// next snapshot to find their row by the drill's key (a view switch
    /// still on its way, a view they are not on). Off, on the list, or
    /// mid-comparison without their row, nothing happens. Opt-in: the TUI
    /// never calls it, so its semantics are untouched.
    pub fn select_player(&mut self, key: &str, label: &str) -> Vec<ClientMsg> {
        if let Some(row) = self.rows().iter().position(|r| r.key == key) {
            return self.select_row(row);
        }
        if !self.follow || self.screen != Screen::Meter || !self.follow_drill(key, label) {
            return Vec::new();
        }
        // A player named from outside the meter's rows is shown, not
        // inspected: the keys stay on the rows.
        self.inspecting = false;
        vec![self.watch_msg()]
    }

    /// Following: `v` on the selected row. With nothing pinned it pins the
    /// selection — the pair's first half, `compare_picks()[0]` — and the
    /// next move makes the second; with a pin (or a pair) it stops
    /// comparing altogether, whichever row the selection is on.
    fn toggle_pin(&mut self) -> Vec<ClientMsg> {
        // R24: enemies are not compared.
        if self.view == View::EnemyTaken {
            return Vec::new();
        }
        if !self.compare.is_empty() {
            self.compare_spell = None;
            let comparing = self.screen == Screen::Compare;
            self.compare.clear();
            self.forget_compare_snap();
            self.screen = Screen::Meter;
            // A lone pin changed nothing the daemon was asked for.
            return if comparing {
                vec![self.watch_msg()]
            } else {
                Vec::new()
            };
        }
        let Some(row) = self.rows().get(self.row_sel).cloned() else {
            return Vec::new();
        };
        self.compare = vec![(row.key, row.label)];
        self.follow_sync()
    }

    /// Following: bring the drill — or the pair — into line with the
    /// selection, and say so to the daemon when it moved. The selected row
    /// is the drill; with a pin on another player the two are compared
    /// (`Screen::Compare`, the meter's rows kept as they last stood, since
    /// the comparison's cursor carries none); on the pin itself the pin is
    /// inspected alone. Nothing happens without rows to select from.
    fn follow_sync(&mut self) -> Vec<ClientMsg> {
        if !self.follow || self.screen == Screen::List {
            return Vec::new();
        }
        let rows = self.rows();
        let Some(row) = rows.get(self.row_sel) else {
            // An answered view with nobody in it (no one dispelled, no one
            // died): no player to follow, so the last one's drill goes
            // rather than wait for numbers that are not coming. Unanswered,
            // the view's rows are merely on their way.
            if self.screen == Screen::Meter && self.view_answered() && self.drill.take().is_some() {
                self.inspecting = false;
                self.death = None;
                self.drill_range = None;
                return vec![self.watch_msg()];
            }
            return Vec::new();
        };
        let pin = self.compare.first().cloned();
        match pin.filter(|(a, _)| *a != row.key) {
            Some(a) => {
                let b = (row.key.clone(), row.label.clone());
                self.follow_drill(&row.key, &row.label);
                if self.screen == Screen::Compare && self.compare.get(1) == Some(&b) {
                    return Vec::new();
                }
                self.compare = vec![a, b];
                self.forget_compare_snap();
                self.compare_spell = None;
                self.screen = Screen::Compare;
                vec![self.watch_msg()]
            }
            None => {
                let was_pair = self.screen == Screen::Compare;
                if was_pair {
                    self.screen = Screen::Meter;
                    self.compare.truncate(1);
                    self.forget_compare_snap();
                    self.compare_spell = None;
                }
                if self.follow_drill(&row.key, &row.label) || was_pair {
                    vec![self.watch_msg()]
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// Point the drill at the player `key` (`label` their name), keeping
    /// which pane it shows (the enemies' drill has one); `true` when
    /// that changed whose drill it is. The breakdown in hand is the last
    /// player's, so it goes: a moment with no drill on screen beats one
    /// with the wrong player's under the new name. That is all this can
    /// promise — a push for the last player already in flight still lands
    /// as this one's until the Watch's reply replaces it (see `on_msg`).
    fn follow_drill(&mut self, key: &str, label: &str) -> bool {
        if self.drill.as_ref().is_some_and(|d| d.key == key) {
            return false;
        }
        let pane = match (self.view, self.drill.as_ref()) {
            // R24: the enemy drill has one list, the attackers.
            (View::EnemyTaken, _) => Pane::Target,
            (_, Some(d)) => d.pane,
            (_, None) => Pane::Spell,
        };
        self.drill = Some(Drill {
            key: key.to_string(),
            label: label.to_string(),
            pane,
            spell_sel: 0,
            target_sel: 0,
            spell: None,
        });
        self.death = None;
        self.drill_range = None;
        if let Some(s) = self.snapshot.as_mut() {
            s.breakdown = None;
        }
        true
    }

    /// Following, a snapshot re-sorts the rows under the selection: find
    /// the drilled player again by key, so the highlight and the drill
    /// stay on one person, then settle anything the new rows changed (a
    /// player with no row in this view, a pair to re-form after a move).
    fn follow_snapshot(&mut self) -> Vec<ClientMsg> {
        if let Some(key) = self.drill.as_ref().map(|d| d.key.clone())
            && let Some(i) = self.rows().iter().position(|r| r.key == key)
        {
            self.row_sel = i;
        }
        self.follow_sync()
    }

    /// Following, a move to another segment keeps the drilled player —
    /// the next pull is most likely theirs too — and the pin, and closes
    /// everything that belonged to the fight left behind: the ability, the
    /// death window, the zoom and the pair's answer. The first snapshot
    /// finds the player again, or settles on another row.
    fn follow_to_segment(&mut self) {
        self.screen = Screen::Meter;
        self.compare.truncate(1);
        self.forget_compare_snap();
        self.compare_spell = None;
        if let Some(d) = self.drill.as_mut() {
            d.spell = None;
            d.spell_sel = 0;
            d.target_sel = 0;
        }
        self.death = None;
        self.drill_range = None;
        self.row_sel = 0;
        self.snapshot = None;
    }

    /// Drop the comparison's answer and zoom — the pair changed, or went.
    fn forget_compare_snap(&mut self) {
        self.compare_snap = None;
        self.compare_range = None;
        self.compare_snap_range = None;
        self.compare_snap_view = None;
    }

    // ---- accessors (the old `App` surface) ----------------------------------

    /// Does the last snapshot answer the view on screen? `false` between a
    /// `SetView` and its reply, when [`ClientState::rows`] is empty for
    /// want of an answer rather than because nothing happened — which a
    /// renderer that sums the rows must not show as a confident zero.
    /// Additive: nothing here reads it.
    pub fn view_answered(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.view == self.view)
    }

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
            }) if *view == self.view && self.range_matches(b) => {
                (b.by_spell.clone(), b.by_target.clone())
            }
            _ => (Vec::new(), Vec::new()),
        }
    }

    /// v33 (R24): on a view that windows its drill a breakdown answers ONE
    /// window; a snapshot still in flight from before a zoom must not show
    /// as if it were the window's.
    fn range_matches(&self, b: &Breakdown) -> bool {
        !self.drill_windowed() || b.range == self.drill_range
    }

    /// v38: does the drill on show ride its zoom window to the daemon — a
    /// drill open on a view that windows one, against a daemon that does?
    fn drill_windowed(&self) -> bool {
        self.drill.is_some()
            && self.view.windows_drill()
            && (self.drill_windows || self.view == View::EnemyTaken)
    }

    /// v38: tell this state whether the daemon behind it answers a drill's
    /// zoom window. A stored pull's says no: its state then never sends a
    /// window, and the rows it shows are the whole pull.
    pub fn set_drill_windows(&mut self, on: bool) {
        self.drill_windows = on;
    }

    /// v38: the window the drill's rows on show answer — the breakdown's
    /// echo, so a reply in flight never words itself as the new window's.
    /// `None` is the whole fight: no zoom, a view whose drill is never
    /// windowed, or a daemon that windows none.
    pub fn drill_shown_range(&self) -> Option<(u32, u32)> {
        self.drill_breakdown().and_then(|b| b.range)
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

    /// v21 (R17): the drilled player's mitigation record, when the snapshot
    /// carries one (Taken view only).
    /// v36 (R26): how the drilled player's by-ability rows nest — the
    /// groups, each row's casts and parts. Empty with no drill, on a view
    /// without one, and until the drilled snapshot arrives.
    pub fn drill_tree(&self) -> SpellTree {
        if self.drill.is_none() {
            return SpellTree::default();
        }
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => b.tree.clone(),
            _ => SpellTree::default(),
        }
    }

    /// v36 (R26 step 2): the drilled snapshot's stacked series — the tree's
    /// largest entries, and an open ability's largest targets — each empty
    /// with no drill, before the drilled snapshot, and for a session the
    /// daemon builds none for.
    pub fn drill_series(&self) -> (&[AbilitySeries], &[AbilitySeries]) {
        if self.drill.is_none() {
            return (&[], &[]);
        }
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => (&b.ability_series, &b.target_series),
            _ => (&[], &[]),
        }
    }

    pub fn drill_mitigation(&self) -> Option<&Mitigation> {
        self.drill.as_ref()?;
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view => b.mitigation.as_ref(),
            _ => None,
        }
    }

    /// How many snapshots have been taken in — it moves whenever the rows
    /// and the breakdown in hand may have.
    pub fn snapshot_gen(&self) -> u64 {
        self.snapshot_gen
    }

    /// The drilled player's whole breakdown, when the snapshot carries one
    /// for the current view. Every drill-side accessor reads through here.
    pub fn drill_breakdown(&self) -> Option<&Breakdown> {
        self.drill.as_ref()?;
        match &self.snapshot {
            Some(Snap {
                view,
                breakdown: Some(b),
                ..
            }) if *view == self.view && self.range_matches(b) => Some(b),
            _ => None,
        }
    }

    /// v28 (R9): the drilled player's death windows, oldest first, and
    /// which one the recap on screen describes (`Breakdown::death_index`,
    /// the daemon's answer — not the request, which may still be in
    /// flight). Empty off the Deaths view and for a survivor.
    pub fn deaths(&self) -> (&[DeathWindow], Option<u32>) {
        match self.drill_breakdown() {
            Some(b) if self.view == View::Deaths => (&b.deaths, b.death_index),
            _ => (&[], None),
        }
    }

    /// v28: ask for another of the drilled player's death windows. `None`
    /// is the last. The request is sent only on the Deaths view with a
    /// drill open; the recap updates when the daemon answers.
    pub fn select_death(&mut self, index: Option<u32>) -> Vec<ClientMsg> {
        if self.view != View::Deaths || self.drill.is_none() || self.death == index {
            return Vec::new();
        }
        self.death = index;
        vec![self.watch_msg()]
    }

    /// v28: the death window the next Watch asks for.
    pub fn death_request(&self) -> Option<u32> {
        self.death
    }

    /// v35 (R25): the watched segment's raid timeline, as the last meter
    /// snapshot carried it. It answers for the SEGMENT, whichever view it
    /// was asked on (`RaidTimeline::view` says which series it holds), so
    /// it is kept across a view switch and under a comparison until the
    /// next meter snapshot replaces it. Additive: nothing here reads it.
    pub fn raid(&self) -> Option<&RaidTimeline> {
        self.snapshot.as_ref()?.raid.as_ref()
    }

    /// v35 (R25): open one death — a raid timeline's skull, a row of the
    /// window's chronological deaths — as the Deaths view drilled into
    /// `key` at its death window `index`. A comparison goes (a recap is
    /// about one player), the drill keeps its pane when it already was
    /// this player's, and the selection moves to their Deaths row when the
    /// rows are in hand (following, the next snapshot finds it by key
    /// otherwise). Opt-in: the window calls it, the TUI never does.
    pub fn open_death(&mut self, key: &str, label: &str, index: u32) -> Vec<ClientMsg> {
        if self.screen == Screen::List {
            return Vec::new();
        }
        // A comparison goes — and whatever else matches, the daemon is still
        // on the comparison's cursor then, so the meter's must be asked for.
        let dropped = !self.compare.is_empty() || self.screen == Screen::Compare;
        if dropped {
            self.compare.clear();
            self.compare_spell = None;
            self.forget_compare_snap();
            self.screen = Screen::Meter;
        }
        let same = self.view == View::Deaths
            && self.drill.as_ref().is_some_and(|d| d.key == key)
            && self.death == Some(index);
        if same && !dropped {
            return Vec::new();
        }
        self.view = View::Deaths;
        match self.drill.as_mut() {
            Some(d) if d.key == key => d.spell = None,
            _ => {
                self.drill = Some(Drill {
                    key: key.to_string(),
                    label: label.to_string(),
                    pane: Pane::Spell,
                    spell_sel: 0,
                    target_sel: 0,
                    spell: None,
                });
                if let Some(s) = self.snapshot.as_mut() {
                    s.breakdown = None;
                }
            }
        }
        self.drill_range = None;
        self.death = Some(index);
        if let Some(i) = self.rows().iter().position(|r| r.key == key) {
            self.row_sel = i;
        }
        vec![self.watch_msg()]
    }

    /// v27 (R21): the drilled player's stack ledger — the hostile debuffs
    /// seen open on them, the cells behind them and the per-spell baseline
    /// the reader derives level 0 from. Empty off the Taken view.
    pub fn drill_stacks(&self) -> Option<(&[StackingDebuff], &[StackCell], &[StackBase])> {
        let b = self.drill_breakdown()?;
        (self.view == View::Taken).then_some((&b.stacking, &b.stacks, &b.stack_base))
    }

    /// v14: the drill graph's zoom window, as asked. The timeline is whole,
    /// so the renderer slices it itself; the rows' own window is
    /// [`Self::drill_shown_range`].
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
        let (by_spell, by_target) = self.breakdown();
        // R24: on the enemy view the drilled "spell" is an attacker row.
        let list = if self.view == View::EnemyTaken {
            by_target
        } else {
            by_spell
        };
        list.into_iter().find(|r| r.key == key)
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
            }) if *view == self.view && self.range_matches(b) => {
                b.spell_targets.clone().unwrap_or_default()
            }
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

    /// v33 (R24): where the view windows its drill (the enemies' attackers,
    /// v33; Damage's and Healing's abilities, v38) the window also scopes
    /// the drill's rows, so a changed window re-watches; every other view
    /// zooms the curve client-side and sends nothing.
    pub fn set_drill_range(&mut self, range: Option<(u32, u32)>) -> Vec<ClientMsg> {
        // A degenerate selection means zoom out, like the comparison's.
        let range = range.filter(|(lo, hi)| lo < hi);
        let changed = range != self.drill_range;
        self.drill_range = range;
        if changed && self.drill_windowed() {
            vec![self.watch_msg()]
        } else {
            Vec::new()
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

    /// v20: the watched boss pull's ENCOUNTER_START identity — its
    /// difficulty and group size, for a header that words them. `None` off
    /// raid-boss encounters (see [`SegmentInfo::encounter`]).
    pub fn segment_encounter(&self) -> Option<Encounter> {
        self.snapshot.as_ref().and_then(|s| s.info.encounter)
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

    /// v20: the tailed log's identity (`proto::history::log_id`), once the
    /// daemon has named it — what turns a list row into the fight id the
    /// history store files it under (`history::fight_id`), so a client
    /// listing stored fights beside the log's can tell the two apart.
    pub fn log_id(&self) -> Option<u64> {
        self.log_id
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
        if self.follow {
            self.follow_to_segment();
            self.cursor = SegmentRef::Live;
            return vec![self.watch_msg()];
        }
        // R12: returning to live keeps an open comparison, like any other
        // segment move — the pair follows onto the live fight.
        if comparing {
            self.forget_compare_snap();
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
                raid,
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
                // Following, a push for the last player that was already in
                // flight when the selection moved is taken for this one's:
                // a `Snapshot` carries no drill key to tell them apart. The
                // Watch's immediate reply follows it on the same socket and
                // replaces it, so the wrong breakdown lives one message.
                self.snapshot = Some(Snap {
                    view,
                    info,
                    rows,
                    breakdown,
                    segment_count,
                    raid,
                });
                self.snapshot_gen = self.snapshot_gen.wrapping_add(1);
                self.clamp_selection();
                if self.follow {
                    return self.follow_snapshot();
                }
                Vec::new()
            }
            DaemonMsg::SegmentList {
                entries,
                source,
                active,
                log_id,
                ..
            } => {
                if self.rotated(&source) {
                    return self.reset_for_new_source(source);
                }
                self.source = source;
                self.log_id = log_id;
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
                view,
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
                        raid: None,
                    });
                }
                self.compare_snap = Some((*a, *b));
                self.compare_snap_range = range;
                self.compare_snap_view = Some(view);
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
            // An opt-in outlives the session it was made in.
            follow: self.follow,
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
        if self.follow
            && let Some(sent) = self.apply_compare_following(action)
        {
            return sent;
        }
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
            // v29: the comparison follows the view, so the view keys work
            // here too — switching to Taken re-asks for the same pair on
            // what hit them, without breaking the pick.
            // R24: no comparison on the enemy view — its rows are enemies, not
            // players — so a view switch to it is refused while comparing.
            Action::SetView(view) if view != self.view && view != View::EnemyTaken => {
                self.view = view;
                self.compare_spell = None;
                self.compare_range = None;
                vec![self.watch_msg()]
            }
            Action::Back | Action::PickCompare => self.clear_compare(),
            _ => Vec::new(),
        }
    }

    /// Following, the comparison sits beside the meter rather than over
    /// it: j/k still move the selection — the pair's second half — Enter
    /// and Back step into and out of the inspector, `v` stops comparing,
    /// and a view switch fetches the new view's rows before the pair
    /// re-forms on them (the comparison's cursor carries no rows). `None`
    /// leaves the action to the comparison's own handling.
    fn apply_compare_following(&mut self, action: Action) -> Option<Vec<ClientMsg>> {
        Some(match action {
            Action::Up => self.step_row(-1),
            Action::Down => self.step_row(1),
            Action::Open => {
                self.inspect();
                Vec::new()
            }
            // One level at a time: the pair's ability (the comparison's own
            // Back), then the keys, then the pair.
            Action::Back if self.compare_spell.is_some() => return None,
            Action::Back if self.inspecting => {
                self.inspecting = false;
                Vec::new()
            }
            Action::PickCompare => self.toggle_pin(),
            Action::SetView(view) if view != self.view => {
                self.view = view;
                self.compare_spell = None;
                self.forget_compare_snap();
                if view == View::EnemyTaken {
                    // R24: enemies are not compared, and the drill's key (a
                    // guid) names no enemy — both go.
                    self.compare.clear();
                    self.drill = None;
                } else {
                    self.compare.truncate(1);
                    if let Some(d) = self.drill.as_mut() {
                        d.spell = None;
                    }
                }
                self.death = None;
                self.drill_range = None;
                self.screen = Screen::Meter;
                vec![self.watch_msg()]
            }
            _ => return None,
        })
    }

    /// Following, the meter keeps its selection's drill open: j/k move the
    /// selection and the drill follows, until Enter hands them to the
    /// drill's panes (where Enter opens the ability, as it always did);
    /// Back closes the ability, then hands the keys back, then leaves for
    /// the list; `v` pins. `None` leaves the action to the meter's own
    /// handling.
    fn apply_meter_following(&mut self, action: Action) -> Option<Vec<ClientMsg>> {
        Some(match action {
            Action::Up if !self.inspecting => self.step_row(-1),
            Action::Down if !self.inspecting => self.step_row(1),
            Action::Open if !self.inspecting => {
                self.inspect();
                Vec::new()
            }
            Action::Back if self.drill.as_ref().is_some_and(|d| d.spell.is_some()) => return None,
            Action::Back if self.inspecting => {
                self.inspecting = false;
                Vec::new()
            }
            Action::Back => {
                self.list_sel = self
                    .segment_index()
                    .min(self.entries.len().saturating_sub(1));
                self.screen = Screen::List;
                self.drill = None;
                self.drill_range = None;
                self.death = None;
                self.compare.clear();
                vec![self.watch_msg()]
            }
            Action::PickCompare => self.toggle_pin(),
            Action::SetView(view) => {
                // R24: a pin is a player; on the enemies' rows it would pair
                // with an enemy. The meter's own switch does the rest.
                if view == View::EnemyTaken {
                    self.compare.clear();
                }
                return None;
            }
            _ => return None,
        })
    }

    /// Following: move the meter's selection by `delta`, clamped, and let
    /// the drill (or the pair) follow it.
    fn step_row(&mut self, delta: isize) -> Vec<ClientMsg> {
        let len = self.rows().len();
        if len == 0 {
            return Vec::new();
        }
        self.row_sel = self.row_sel.saturating_add_signed(delta).min(len - 1);
        self.follow_sync()
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
        if self.follow
            && let Some(sent) = self.apply_meter_following(action)
        {
            return sent;
        }
        match action {
            Action::Quit => {
                self.quit = true;
                Vec::new()
            }
            Action::SetView(view) => {
                // R24: an enemy drill is keyed by NAME, a player drill by guid;
                // across that boundary the key answers nothing, so the drill
                // closes rather than survive as an empty screen.
                let keyspace_changes =
                    (self.view == View::EnemyTaken) != (view == View::EnemyTaken);
                self.view = view;
                if keyspace_changes {
                    self.drill = None;
                    self.drill_range = None;
                }
                // The drilldown follows the player across views, like always
                // — but not the ABILITY drill: by-spell keys are view-local
                // ("Flash Heal" is not a damage row), so it closes (v16).
                if let Some(d) = self.drill.as_mut() {
                    d.spell = None;
                    self.drill_range = None;
                }
                self.death = None;
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
                // R24: enemies are not compared.
                if self.view == View::EnemyTaken {
                    return Vec::new();
                }
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
                    if self.view != View::EnemyTaken {
                        self.drill_range = None;
                    }
                    vec![self.watch_msg()]
                } else if self.drill.is_some() {
                    self.drill = None;
                    self.drill_range = None;
                    self.death = None;
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
                // R24: the enemy drill has one pane; there is nothing to swap to.
                if self.view != View::EnemyTaken
                    && let Some(drill) = self.drill.as_mut()
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
        if self.follow {
            self.follow_to_segment();
            return vec![self.watch_msg()];
        }
        // R12: segment navigation never breaks an open comparison — the
        // pair sticks and the new segment's sides are requested for it. The
        // stale sides are dropped rather than shown under the new header.
        if self.screen == Screen::Compare && self.compare.len() == 2 {
            self.forget_compare_snap();
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
        self.death = None;
        self.drill = Some(Drill {
            key: row.key.clone(),
            label: row.label.clone(),
            // R24: the enemy drill has one list, the attackers.
            pane: if self.view == View::EnemyTaken {
                Pane::Target
            } else {
                Pane::Spell
            },
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
        // R24: the enemy drill descends from the ATTACKER pane — the second
        // level is that attacker's abilities, keyed by their name.
        let enemy = self.view == View::EnemyTaken;
        if !enemy && !matches!(self.view, View::Damage | View::Healing) {
            return Vec::new();
        }
        let (by_spell, by_target) = self.breakdown();
        let Some(drill) = self.drill.as_mut() else {
            return Vec::new();
        };
        if drill.spell.is_some() {
            return Vec::new();
        }
        let row = if enemy {
            if drill.pane != Pane::Target {
                return Vec::new();
            }
            by_target.get(drill.target_sel)
        } else {
            if drill.pane != Pane::Spell {
                return Vec::new();
            }
            by_spell.get(drill.spell_sel)
        };
        let Some(row) = row else {
            return Vec::new();
        };
        drill.spell = Some((row.key.clone(), row.label.clone()));
        // v33 (R24): the enemy drill keeps its zoom window into the attacker
        // level — "what did they do in THIS window" is the question.
        if !enemy {
            self.drill_range = None;
        }
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
            raid: None,
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

    /// v28: a death window is chosen through the state, named on the Watch,
    /// and forgotten when the drill or the view changes.
    #[test]
    fn a_chosen_death_window_rides_the_watch_and_resets_with_the_drill() {
        let mut st = ClientState::new();
        st.screen = Screen::Meter;
        st.view = View::Deaths;
        assert!(
            st.select_death(Some(1)).is_empty(),
            "no drill, nothing to ask"
        );
        st.drill = Some(Drill {
            key: "p".into(),
            label: "P".into(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
            spell: None,
        });
        let sent = st.select_death(Some(1));
        assert!(matches!(
            sent.as_slice(),
            [ClientMsg::Watch(Cursor::Segment { death: Some(1), .. })]
        ));
        assert!(st.select_death(Some(1)).is_empty(), "already asked");
        assert_eq!(st.death_request(), Some(1));
        st.apply(Action::SetView(View::Damage));
        assert_eq!(st.death_request(), None, "a view change forgets it");
        st.view = View::Deaths;
        st.select_death(Some(2));
        st.apply(Action::Back);
        assert!(st.drill.is_none());
        assert_eq!(st.death_request(), None, "closing the drill forgets it");
        // Off the Deaths view the selection is refused outright.
        st.view = View::Taken;
        st.drill = Some(Drill {
            key: "p".into(),
            label: "P".into(),
            pane: Pane::Spell,
            spell_sel: 0,
            target_sel: 0,
            spell: None,
        });
        assert!(st.select_death(Some(0)).is_empty());
        assert!(st.deaths().0.is_empty());
        assert!(
            st.drill_stacks().is_none(),
            "no snapshot yet, so no ledger to answer from"
        );
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
            raid: None,
        }
    }

    /// Between asking for a view and its reply, the rows are empty for
    /// want of an answer, and the state says so.
    #[test]
    fn a_view_is_answered_only_once_its_snapshot_is_in() {
        let mut st = ClientState::new();
        st.screen = Screen::Meter;
        assert!(!st.view_answered(), "nothing in yet");
        st.on_msg(snap(None));
        assert!(st.view_answered());
        st.apply(Action::SetView(View::Healing));
        assert!(!st.view_answered(), "Damage's reply is no Healing");
        assert!(st.rows().is_empty());
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
            stacking: Vec::new(),
            stacks: Vec::new(),
            stacks_dropped: 0,
            stack_base: Vec::new(),
            deaths: Vec::new(),
            death_index: None,
            deaths_dropped: 0,
            range: None,
            tree: Default::default(),
            ability_series: Vec::new(),
            target_series: Vec::new(),
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

    // ---- follow-selection ---------------------------------------------------

    /// A snapshot of the live segment — the newer of two — in `view`,
    /// whose rows are `keys` in that order, with `breakdown` for whatever
    /// drill was asked.
    fn rows_snap(view: View, keys: &[&str], breakdown: Option<Breakdown>) -> DaemonMsg {
        DaemonMsg::Snapshot {
            seq: 1,
            segment: SegmentRef::Live,
            id: None,
            view,
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
            rows: keys.iter().map(|k| plain_row(k)).collect(),
            total_rows: keys.len() as u32,
            breakdown,
            segment_count: 2,
            source: None,
            status: None,
            raid: None,
        }
    }

    fn one_spell() -> Breakdown {
        Breakdown {
            by_spell: vec![plain_row("Fireball"), plain_row("Scorch")],
            ..Breakdown::default()
        }
    }

    /// The single Watch in `sent`: a meter's drill, or a comparison's pair.
    fn watched(sent: &[ClientMsg]) -> (Option<String>, Option<(String, String)>) {
        match sent {
            [ClientMsg::Watch(Cursor::Segment { drill, .. })] => (drill.clone(), None),
            [ClientMsg::Watch(Cursor::Compare { a, b, .. })] => {
                (None, Some((a.clone(), b.clone())))
            }
            other => panic!("expected one Watch, got {other:?}"),
        }
    }

    /// A meter over rows A, B, C, following.
    fn following() -> ClientState {
        let mut st = ClientState::new();
        st.screen = Screen::Meter;
        st.on_msg(rows_snap(View::Damage, &["A", "B", "C"], None));
        let sent = st.set_follow(true);
        assert_eq!(
            watched(&sent).0.as_deref(),
            Some("A"),
            "the selection's drill"
        );
        st
    }

    /// The TUI never opts in, and without the opt-in the meter's keys mean
    /// what they always did: j moves the highlight and asks for nothing,
    /// Enter opens a drill screen, Esc closes it, `v` picks one of two.
    #[test]
    fn without_the_opt_in_the_meter_keeps_its_drill_screen() {
        let mut st = ClientState::new();
        assert!(!st.follows_selection(), "off unless asked for");
        st.screen = Screen::Meter;
        st.on_msg(rows_snap(View::Damage, &["A", "B", "C"], None));
        assert!(st.apply(Action::Down).is_empty(), "a move is local");
        assert_eq!((st.row_sel, st.drill.is_none()), (1, true));
        assert_eq!(watched(&st.apply(Action::Open)).0.as_deref(), Some("B"));
        st.inspect();
        assert!(!st.inspecting(), "nothing to inspect without the opt-in");
        assert_eq!(watched(&st.apply(Action::Back)).0, None, "Esc closes it");
        assert!(st.select_row(2).is_empty(), "a click only selects");
        assert!(st.drill.is_none());
        st.apply(Action::PickCompare);
        assert_eq!(st.compare_picks().len(), 1, "v picks, as it did");
        assert_eq!(st.screen, Screen::Meter);
        st.apply(Action::Up);
        let sent = st.apply(Action::PickCompare);
        assert_eq!(watched(&sent).1, Some(("C".into(), "B".into())));
        assert_eq!(st.screen, Screen::Compare, "the second pick opens it");
    }

    /// Following, an answered view with nobody in it (no one dispelled)
    /// has no player to follow: the last one's drill goes, the Watch says
    /// so — and the first row a later snapshot brings is followed again.
    /// Without the opt-in the drill is the reader's and stays.
    #[test]
    fn an_empty_view_drops_the_followed_drill() {
        let mut st = following();
        st.apply(Action::Down);
        st.inspect();
        let sent = st.on_msg(rows_snap(View::Damage, &[], None));
        assert_eq!(watched(&sent).0, None, "the drill goes");
        assert!(st.drill.is_none() && !st.inspecting());
        let sent = st.on_msg(rows_snap(View::Damage, &["C"], None));
        assert_eq!(watched(&sent).0.as_deref(), Some("C"), "followed again");

        let mut tui = ClientState::new();
        tui.screen = Screen::Meter;
        tui.on_msg(rows_snap(View::Damage, &["A"], None));
        tui.apply(Action::Open);
        assert!(tui.on_msg(rows_snap(View::Damage, &[], None)).is_empty());
        assert!(tui.drill.is_some(), "the TUI's drill is untouched");
    }

    /// Following, every move of the selection re-watches the segment with
    /// the selected row as the drill — and the screen stays the meter.
    #[test]
    fn following_the_selection_re_watches_its_drill() {
        let mut st = following();
        let sent = st.apply(Action::Down);
        assert_eq!(watched(&sent).0.as_deref(), Some("B"));
        assert_eq!(st.screen, Screen::Meter);
        assert_eq!(st.drill.as_ref().map(|d| d.key.as_str()), Some("B"));
        assert!(
            st.apply(Action::Down).len() == 1 && st.apply(Action::Down).is_empty(),
            "the last row is as far as it goes"
        );
        assert_eq!(watched(&st.select_row(0)).0.as_deref(), Some("A"));
        assert!(st.select_row(0).is_empty(), "already there");
        // Each snapshot now carries the rows AND the drill's breakdown.
        st.on_msg(rows_snap(View::Damage, &["A", "B", "C"], Some(one_spell())));
        assert_eq!(st.rows().len(), 3);
        assert_eq!(st.breakdown().0.len(), 2);
        // The breakdown in hand is A's: a move drops it with the drill, at
        // once — a push for A still in flight is another matter (on_msg).
        let seen = st.snapshot_gen();
        st.apply(Action::Down);
        assert!(st.breakdown().0.is_empty(), "A's panes dropped at the move");
        assert_eq!(st.snapshot_gen(), seen, "a move takes nothing in");
        st.on_msg(rows_snap(View::Damage, &["A", "B", "C"], Some(one_spell())));
        assert_eq!(st.snapshot_gen(), seen + 1, "a snapshot does");
        assert!(st.set_follow(true).is_empty(), "already on");
    }

    /// The window's command palette names a player, not a row: their row
    /// when the chart in hand has one; else the drill put on them, for the
    /// next snapshot to find their row by. Opt-in — without the follow it
    /// only selects a row it finds, so the TUI never meets the rest.
    #[test]
    fn a_player_is_selected_by_key() {
        let mut st = following();
        let sent = st.select_player("C", "C");
        assert_eq!(watched(&sent).0.as_deref(), Some("C"), "their row");
        assert_eq!(st.row_sel, 2);
        // A view switch on its way: the chart in hand is not the view's,
        // so the drill goes on them and the Watch says so.
        st.apply(Action::SetView(View::Healing));
        assert!(st.rows().is_empty(), "the view's rows are on their way");
        let sent = st.select_player("B", "B");
        assert_eq!(watched(&sent).0.as_deref(), Some("B"), "the drill named");
        assert!(st.select_player("B", "B").is_empty(), "already theirs");
        // …and the snapshot finds their row.
        st.on_msg(rows_snap(View::Healing, &["C", "A", "B"], None));
        assert_eq!(st.row_sel, 2);
        assert_eq!(st.drill.as_ref().map(|d| d.key.as_str()), Some("B"));

        let mut tui = ClientState::new();
        tui.screen = Screen::Meter;
        tui.on_msg(rows_snap(View::Damage, &["A", "B"], None));
        assert!(tui.select_player("B", "B").is_empty(), "no Watch unasked");
        assert_eq!((tui.row_sel, tui.drill.is_none()), (1, true));
        assert!(tui.select_player("Z", "Z").is_empty() && tui.drill.is_none());
    }

    /// Rows re-sort under the selection between snapshots; the drill is
    /// keyed by guid, so the highlight follows the player, not the index.
    #[test]
    fn a_resort_keeps_the_selection_on_the_drilled_player() {
        let mut st = following();
        st.apply(Action::Down);
        let sent = st.on_msg(rows_snap(View::Damage, &["B", "A", "C"], None));
        assert!(sent.is_empty(), "nothing changed but the order");
        assert_eq!(st.row_sel, 0, "B is first now");
        // A player the new rows do not have hands the drill to the
        // selection's row, and asks for its breakdown.
        let sent = st.on_msg(rows_snap(View::Damage, &["A", "C"], None));
        assert_eq!(watched(&sent).0.as_deref(), Some("A"));
    }

    /// Enter hands the keys to the drill's panes (a narrow window pushes the
    /// inspector); there j/k walk the abilities, Enter opens one, and Back
    /// pops one level at a time before the keys come back to the meter.
    #[test]
    fn enter_hands_the_keys_to_the_drill_and_back_returns_them() {
        let mut st = following();
        st.on_msg(rows_snap(View::Damage, &["A", "B", "C"], Some(one_spell())));
        assert!(st.apply(Action::Open).is_empty(), "no new cursor");
        assert!(st.inspecting());
        assert!(st.apply(Action::Down).is_empty(), "a pane move is local");
        assert_eq!(st.drill.as_ref().map(|d| d.spell_sel), Some(1));
        assert_eq!(st.row_sel, 0, "the meter's selection stays put");
        let sent = st.apply(Action::Open);
        assert!(
            matches!(&sent[..], [ClientMsg::Watch(Cursor::Segment { spell: Some(s), .. })] if s == "Scorch")
        );
        st.apply(Action::Back);
        assert!(
            st.drill_spell().is_none() && st.inspecting(),
            "the ability first"
        );
        assert!(st.apply(Action::Back).is_empty());
        assert!(!st.inspecting(), "then the keys");
        assert_eq!(watched(&st.apply(Action::Down)).0.as_deref(), Some("B"));
        // And from the meter itself, Back leaves for the list.
        let sent = st.apply(Action::Back);
        assert!(matches!(&sent[..], [ClientMsg::Watch(Cursor::List)]));
        assert!(st.drill.is_none() && st.screen == Screen::List);
    }

    /// `v` pins the selected player; moving the selection makes the pair,
    /// back onto the pin inspects it alone, and `v` again stops it all.
    #[test]
    fn v_pins_and_the_selection_is_the_pair_s_other_half() {
        let mut st = following();
        assert!(
            st.apply(Action::PickCompare).is_empty(),
            "a pin asks nothing"
        );
        assert_eq!(st.compare_picks(), &[("A".to_string(), "A".to_string())]);
        assert_eq!(st.screen, Screen::Meter);
        let sent = st.apply(Action::Down);
        assert_eq!(watched(&sent).1, Some(("A".into(), "B".into())));
        assert_eq!(st.screen, Screen::Compare);
        assert_eq!(st.row_sel, 1, "the meter's highlight is the second half");
        let sent = st.apply(Action::Down);
        assert_eq!(
            watched(&sent).1,
            Some(("A".into(), "C".into())),
            "B swapped for C"
        );
        let sent = st.select_row(0);
        assert_eq!(watched(&sent).0.as_deref(), Some("A"), "the pin, alone");
        assert_eq!(st.screen, Screen::Meter);
        assert_eq!(st.compare_picks().len(), 1, "still pinned");
        st.apply(Action::Down);
        assert_eq!(st.screen, Screen::Compare);
        let sent = st.apply(Action::PickCompare);
        assert_eq!(watched(&sent).0.as_deref(), Some("B"), "v stops: B's drill");
        assert!(st.compare_picks().is_empty() && st.screen == Screen::Meter);
        // A lone pin stops without asking anything.
        st.apply(Action::PickCompare);
        assert!(st.apply(Action::PickCompare).is_empty());
        assert!(st.compare_picks().is_empty());
    }

    /// Esc in a comparison: the keys first (a narrow window's inspector),
    /// then the pair — and the meter's drill is the selection's again.
    #[test]
    fn back_leaves_the_inspector_before_the_comparison() {
        let mut st = following();
        st.apply(Action::PickCompare);
        st.apply(Action::Down);
        st.apply(Action::Open);
        assert!(st.inspecting(), "a comparison can be inspected too");
        assert!(st.apply(Action::Back).is_empty());
        assert_eq!(st.screen, Screen::Compare, "the pair outlives the keys");
        let sent = st.apply(Action::Back);
        assert_eq!(watched(&sent).0.as_deref(), Some("B"));
        assert!(st.compare_picks().is_empty() && st.screen == Screen::Meter);
    }

    /// The comparison's cursor carries no rows, so a view switch mid-pair
    /// asks for the new view's meter first and the pair re-forms on it.
    #[test]
    fn a_view_switch_mid_pair_fetches_the_rows_before_the_pair() {
        let mut st = following();
        st.apply(Action::PickCompare);
        st.apply(Action::Down);
        let sent = st.apply(Action::SetView(View::Healing));
        assert_eq!(watched(&sent).0.as_deref(), Some("B"), "the meter, drilled");
        assert_eq!(st.screen, Screen::Meter);
        assert_eq!(st.compare_picks().len(), 1, "the pin stays");
        let sent = st.on_msg(rows_snap(View::Healing, &["C", "B", "A"], None));
        assert_eq!(watched(&sent).1, Some(("A".into(), "B".into())));
        assert_eq!(st.row_sel, 1, "B, where Healing ranks them");
        // Enemies are not compared: the pin goes with the switch.
        st.apply(Action::SetView(View::EnemyTaken));
        assert!(st.compare_picks().is_empty() && st.drill.is_none());
    }

    /// A move to another pull keeps the followed player and the pin; the
    /// new rows find them again.
    #[test]
    fn a_segment_move_keeps_the_followed_player() {
        let mut st = following();
        st.on_msg(DaemonMsg::SegmentList {
            seq: 3,
            entries: (0..2)
                .map(|i| ListEntry {
                    id: SegmentId(i),
                    row: ListRow {
                        kind: SegmentKind::Encounter,
                        name: String::new(),
                        start_ms: 0,
                        success: None,
                        duration_ms: 1,
                        live: false,
                        instance: None,
                        pars_ms: None,
                        arena: false,
                        encounter: None,
                    },
                })
                .collect(),
            source: None,
            active: false,
            log_id: None,
        });
        st.apply(Action::Down);
        let sent = st.apply(Action::OlderSegment);
        assert_eq!(
            watched(&sent).0.as_deref(),
            Some("B"),
            "B, into the older pull"
        );
        assert!(st.rows().is_empty(), "the new pull's rows are on their way");
    }

    /// A rotated log starts the session over; the opt-in is not session
    /// state and survives it.
    #[test]
    fn the_opt_in_outlives_a_rotated_log() {
        let mut st = following();
        st.source = Some("a.txt".into());
        st.on_msg(DaemonMsg::SegmentList {
            seq: 9,
            entries: Vec::new(),
            source: Some("b.txt".into()),
            active: false,
            log_id: None,
        });
        assert!(st.follows_selection());
        assert_eq!(st.screen, Screen::List);
    }

    /// The list names the log it came from, and the name is the newest
    /// list's: a client pairs rows with stored fights through it. Taking it
    /// in changes nothing else — the TUI never asks.
    #[test]
    fn the_list_names_its_log() {
        let mut st = ClientState::new();
        assert_eq!(st.log_id(), None, "nothing named yet");
        let list = |log_id| DaemonMsg::SegmentList {
            seq: 1,
            entries: Vec::new(),
            source: Some("a.txt".into()),
            active: false,
            log_id,
        };
        assert!(st.on_msg(list(Some(0xfeed))).is_empty());
        assert_eq!(st.log_id(), Some(0xfeed));
        assert_eq!(st.screen, Screen::List);
        let _ = st.on_msg(list(None));
        assert_eq!(st.log_id(), None, "the header not in yet");
    }
}
