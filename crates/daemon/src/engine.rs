//! The one pipeline instance behind every client: the live meter, the
//! structural index with stable ids, and the LRU of lazily parsed historical
//! segments. Everything `app.rs` used to do per-frontend with
//! `index`/`loaded`/`load_pending` happens here, once.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use wowdps_core::index::SegmentMeta;
use wowdps_core::model::{ListRow, Meter, Row, SegmentId, SegmentInfo, SegmentKind, parse_line};
use wowdps_core::tail::TailEvent;
use wowdps_model::View;
use wowdps_proto::{Breakdown, DaemonMsg, ListEntry, LoadError, SegmentRef};

/// Parsed historical segments kept in memory, across *all* clients. Each is
/// one fight's per-actor hashmaps; the LRU bound is what keeps N clients
/// browsing N different segments from growing the daemon without limit.
pub const LOADED_CAP: usize = 16;

/// Something the hub should tell every session about.
pub enum EngineEvent {
    /// A new segment opened on fresh combat (not backlog replay).
    Opened(SegmentId),
}

/// What building a snapshot for a cursor came to.
pub enum Built {
    /// Push this.
    Ready(Box<DaemonMsg>),
    /// Push this placeholder (info + "loading…"), and get the slice parsed.
    Loading(Box<DaemonMsg>, SegmentId, SegmentMeta),
    /// The cursor points at nothing that can ever load.
    Failed(SegmentId, LoadError),
}

pub struct Engine {
    /// The live meter, fed only the tail from the index's `live_offset`.
    meter: Meter,
    /// Max log timestamp seen — live durations use the log's clock, never the
    /// wall clock.
    now_ms: i64,
    /// Closed historical segments from the scan, oldest first.
    index: Vec<SegmentMeta>,
    index_ids: Vec<SegmentId>,
    /// R10: Overall metas of visits closed before `live_offset`, with ids.
    index_overalls: Vec<SegmentMeta>,
    index_overall_ids: Vec<SegmentId>,
    /// R10: the scan's in-progress visit — the prefix the live meter cannot
    /// see. Its ordinal's id lives in `visit_ids`.
    open_visit: Option<SegmentMeta>,
    /// R10: ids for visits served (at least partly) from the live meter,
    /// keyed by visit ordinal.
    visit_ids: std::collections::HashMap<u32, SegmentId>,
    /// Ids for the live meter's own segments, parallel to `meter.segments()`.
    live_ids: Vec<SegmentId>,
    /// Daemon-lifetime monotonic; never reused, not even across rotation.
    next_id: u64,
    /// First id assigned for the current file: anything below it is Rotated.
    first_id_of_file: u64,
    /// Lazily parsed historical segments, LRU by touch, capped at LOADED_CAP.
    loaded: Vec<(SegmentId, Meter)>,
    /// Loads already handed to the loader pool.
    pub loading: HashSet<SegmentId>,
    pub source_path: Option<PathBuf>,
    pub source_name: Option<String>,
    /// Last tail error, echoed in snapshot footers.
    pub status: Option<String>,
    /// R11: ids the user threw away (the footer trash can) — closed,
    /// out-of-instance Trash only. Tombstones, not deletion: parity state
    /// and Σ merging keep every segment; only the list forgets them.
    discarded: HashSet<SegmentId>,
    /// Backlog drained: segments opening now are fresh combat.
    caught_up: bool,
    seen_segments: usize,
    /// When post-backlog lines last arrived — observation, not file mtime.
    last_fresh: Option<Instant>,
}

/// How recently lines must have arrived for an open segment to count as a
/// fight in progress without the game-process signal.
const FRESH_WINDOW: Duration = Duration::from_secs(10);

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            meter: Meter::new(),
            now_ms: 0,
            index: Vec::new(),
            index_ids: Vec::new(),
            index_overalls: Vec::new(),
            index_overall_ids: Vec::new(),
            open_visit: None,
            visit_ids: std::collections::HashMap::new(),
            live_ids: Vec::new(),
            next_id: 0,
            first_id_of_file: 0,
            loaded: Vec::new(),
            loading: HashSet::new(),
            discarded: HashSet::new(),
            source_path: None,
            source_name: None,
            status: None,
            caught_up: false,
            seen_segments: 0,
            last_fresh: None,
        }
    }

    fn fresh_id(&mut self) -> SegmentId {
        let id = SegmentId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn segment_count(&self) -> usize {
        self.index.len() + self.meter.segments().len()
    }

    /// Combat is happening *now*: an open segment, and either lines arrived
    /// recently or the game process is running. The game-process signal is
    /// what survives the game's multi-minute log flush bursts; the
    /// fresh-lines signal is what works without a game (tests, replays). A
    /// stale file's forever-open trailing segment has neither.
    pub fn live_now(&self, game_running: bool) -> bool {
        let open = self
            .meter
            .segments()
            .last()
            .is_some_and(|s| s.end_ms.is_none());
        let fresh = self.last_fresh.is_some_and(|t| t.elapsed() < FRESH_WINDOW);
        self.caught_up && open && (fresh || game_running)
    }

    pub fn on_tail(&mut self, ev: TailEvent, out: &mut Vec<EngineEvent>) {
        match ev {
            TailEvent::Lines(lines) => {
                if self.caught_up && !lines.is_empty() {
                    self.last_fresh = Some(Instant::now());
                }
                for line in &lines {
                    if let Some(parsed) = parse_line(line) {
                        self.now_ms = self.now_ms.max(parsed.ts_ms);
                        self.meter.feed(parsed);
                    }
                }
                while self.live_ids.len() < self.meter.segments().len() {
                    let id = self.fresh_id();
                    self.live_ids.push(id);
                }
                // R10: a visit whose first member just appeared needs an id
                // so its Overall can be listed and watched.
                let ordinals: Vec<u32> = self
                    .meter
                    .segments()
                    .iter()
                    .filter_map(|s| s.visit)
                    .collect();
                for ord in ordinals {
                    if !self.visit_ids.contains_key(&ord) {
                        let id = self.fresh_id();
                        self.visit_ids.insert(ord, id);
                    }
                }
                let count = self.segment_count();
                let opened = count > self.seen_segments;
                self.seen_segments = count;
                if opened
                    && self.caught_up
                    && self
                        .meter
                        .segments()
                        .last()
                        .is_some_and(|s| s.end_ms.is_none())
                    && let Some(id) = self.live_ids.last()
                {
                    out.push(EngineEvent::Opened(*id));
                }
            }
            TailEvent::CaughtUp => self.caught_up = true,
            TailEvent::Switched(path) => {
                // A different log file is a different session: reset all
                // per-file state. Ids keep counting — that is the whole
                // point of daemon-lifetime monotonicity.
                self.source_name = Some(
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string()),
                );
                self.source_path = Some(path);
                self.meter = Meter::new();
                self.now_ms = 0;
                self.index.clear();
                self.index_ids.clear();
                self.index_overalls.clear();
                self.index_overall_ids.clear();
                self.open_visit = None;
                self.visit_ids.clear();
                self.live_ids.clear();
                self.loaded.clear();
                self.loading.clear();
                self.status = None;
                self.caught_up = false;
                self.seen_segments = 0;
                self.last_fresh = None;
                self.first_id_of_file = self.next_id;
            }
            TailEvent::Index { index, .. } => {
                self.index = index.segments;
                self.index_ids = (0..self.index.len()).map(|_| self.fresh_id()).collect();
                self.index_overalls = index.overalls;
                self.index_overall_ids = (0..self.index_overalls.len())
                    .map(|_| self.fresh_id())
                    .collect();
                // R10: the scan's in-progress visit continues in the live
                // meter — one id covers both halves.
                self.open_visit = index.open_visit;
                if let Some(ord) = self.open_visit.as_ref().and_then(|m| m.visit) {
                    let id = self.fresh_id();
                    self.visit_ids.insert(ord, id);
                }
                self.seen_segments = self.segment_count();
            }
            TailEvent::Waiting => self.source_name = None,
            TailEvent::Error(msg) => self.status = Some(msg),
        }
    }

    /// The combined list, oldest first: indexed history then live segments,
    /// with each visit's Overall row inserted as a header right before the
    /// visit's first member (R10).
    fn list_entries_full(&self) -> Vec<(SegmentId, ListRow)> {
        // R11: closed segments without meaningful activity (no enemy damage,
        // no player death) get no row — but a live segment always shows, so
        // the meter still tracks world healing while it happens.
        let discarded = &self.discarded;
        let indexed = self
            .index
            .iter()
            .zip(&self.index_ids)
            .filter(|(m, id)| m.counts && !discarded.contains(id))
            .map(|(m, id)| {
                (
                    *id,
                    ListRow {
                        kind: m.kind,
                        name: m.name.clone(),
                        start_ms: m.start_ms,
                        success: m.success,
                        duration_ms: m.duration_ms,
                        live: false,
                        instance: m.visit,
                        pars_ms: m.pars_ms,
                    },
                )
            });
        let live = self
            .meter
            .segments()
            .iter()
            .zip(&self.live_ids)
            .filter(|(s, id)| (s.end_ms.is_none() || s.counts()) && !discarded.contains(id))
            .map(|(s, id)| {
                (
                    *id,
                    ListRow {
                        kind: s.kind,
                        name: s.name.clone(),
                        start_ms: s.start_ms,
                        success: s.success,
                        duration_ms: s.duration_ms(self.now_ms),
                        live: s.end_ms.is_none(),
                        instance: s.visit,
                        pars_ms: None,
                    },
                )
            });
        let mut entries: Vec<(SegmentId, ListRow)> = indexed.chain(live).collect();

        // Closed visits from the scan, then visits with live members: each
        // Overall goes right before its first member.
        let mut overalls: Vec<(SegmentId, ListRow)> = self
            .index_overalls
            .iter()
            .zip(&self.index_overall_ids)
            .map(|(m, id)| {
                (
                    *id,
                    ListRow {
                        kind: SegmentKind::Overall,
                        name: m.name.clone(),
                        start_ms: m.start_ms,
                        success: m.success,
                        duration_ms: m.duration_ms,
                        live: false,
                        instance: m.visit,
                        pars_ms: m.pars_ms,
                    },
                )
            })
            .collect();
        let mut live_visits: Vec<(u32, SegmentId)> =
            self.visit_ids.iter().map(|(o, i)| (*o, *i)).collect();
        live_visits.sort_unstable();
        for (ord, id) in live_visits {
            let Some(v) = self.meter.visits().get(ord as usize) else {
                continue;
            };
            overalls.push((
                id,
                ListRow {
                    kind: SegmentKind::Overall,
                    name: v.display_name(),
                    start_ms: v.start_ms,
                    success: v.verdict(self.now_ms),
                    duration_ms: self.live_overall_duration(ord),
                    live: v.end_ms.is_none(),
                    instance: Some(ord),
                    pars_ms: v.pars_ms,
                },
            ));
        }
        for entry in overalls {
            let ord = entry.1.instance;
            // R11: a Σ row only exists in front of a visible member — a
            // visit whose every member was filtered out (or none survived
            // rotation) must not leave a dangling Σ-only block.
            let Some(at) = entries
                .iter()
                .position(|(_, r)| r.kind != SegmentKind::Overall && r.instance == ord)
            else {
                continue;
            };
            entries.insert(at, entry);
        }
        entries
    }

    /// R10: a live visit's Overall clock. A keystone run reads the key
    /// timer straight off the visit; otherwise the scanned prefix (members
    /// closed before `live_offset`) plus every live member's R7 duration.
    fn live_overall_duration(&self, ordinal: u32) -> i64 {
        if let Some(clock) = self
            .meter
            .visits()
            .get(ordinal as usize)
            .and_then(|v| v.key_clock(self.now_ms))
        {
            return clock;
        }
        let prefix = self
            .open_visit
            .as_ref()
            .filter(|m| m.visit == Some(ordinal))
            .map_or(0, |m| m.duration_ms);
        let live: i64 = self
            .meter
            .segments()
            .iter()
            .filter(|s| s.visit == Some(ordinal))
            .map(|s| s.duration_ms(s.last_combat_ms()))
            .sum();
        prefix + live
    }

    /// The segment list, oldest first (see `list_entries_full`).
    pub fn list_rows(&self) -> Vec<ListRow> {
        self.list_entries_full()
            .into_iter()
            .map(|(_, r)| r)
            .collect()
    }

    /// Ids for the combined list, position-aligned with `list_rows`.
    pub fn list_ids(&self) -> Vec<SegmentId> {
        self.list_entries_full()
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// `game_running` comes from the hub's game watcher and feeds the
    /// `active` liveness verdict.
    pub fn build_list(&self, game_running: bool) -> DaemonMsg {
        let entries = self
            .list_ids()
            .into_iter()
            .zip(self.list_rows())
            .map(|(id, row)| ListEntry { id, row })
            .collect();
        DaemonMsg::SegmentList {
            seq: 0,
            entries,
            source: self.source_name.clone(),
            active: self.live_now(game_running),
        }
    }

    pub fn install_loaded(&mut self, id: SegmentId, meter: Meter) {
        self.loaded.retain(|(i, _)| *i != id);
        self.loaded.push((id, meter));
        if self.loaded.len() > LOADED_CAP {
            self.loaded.remove(0);
        }
    }

    /// LRU touch + lookup.
    fn touch_loaded(&mut self, id: SegmentId) -> bool {
        let Some(pos) = self.loaded.iter().position(|(i, _)| *i == id) else {
            return false;
        };
        let entry = self.loaded.remove(pos);
        self.loaded.push(entry);
        true
    }

    /// Build the snapshot for one segment cursor. `seq` is left 0 — the
    /// session assigns it when the push actually happens.
    pub fn build_segment(
        &mut self,
        sref: SegmentRef,
        view: View,
        top_n: Option<u32>,
        drill: Option<&str>,
    ) -> Built {
        enum Pos {
            None,
            Idx(usize),
            Live(usize),
            /// R10: a closed visit's Overall from the scan.
            IdxOverall(usize),
            /// R10: a visit served (at least partly) from the live meter.
            LiveOverall(u32),
        }
        let pos = match sref {
            SegmentRef::Live => {
                if !self.meter.segments().is_empty() {
                    Pos::Live(self.meter.segments().len() - 1)
                } else if !self.index.is_empty() {
                    Pos::Idx(self.index.len() - 1)
                } else {
                    Pos::None
                }
            }
            SegmentRef::Id(id) => {
                if let Some(i) = self.index_ids.iter().position(|x| *x == id) {
                    Pos::Idx(i)
                } else if let Some(i) = self.live_ids.iter().position(|x| *x == id) {
                    Pos::Live(i)
                } else if let Some(i) = self.index_overall_ids.iter().position(|x| *x == id) {
                    Pos::IdxOverall(i)
                } else if let Some(ord) = self
                    .visit_ids
                    .iter()
                    .find_map(|(o, x)| (*x == id).then_some(*o))
                {
                    Pos::LiveOverall(ord)
                } else if id.0 < self.first_id_of_file && id.0 < self.next_id {
                    return Built::Failed(id, LoadError::Rotated);
                } else {
                    return Built::Failed(id, LoadError::NotFound);
                }
            }
        };

        match pos {
            Pos::None => self.empty_built(sref, view, top_n),
            Pos::Live(i) => {
                // The two tables are built in lockstep; a miss can only mean
                // a broken invariant, and an empty snapshot beats a panic.
                let (Some(&id), Some(seg)) = (self.live_ids.get(i), self.meter.segments().get(i))
                else {
                    return self.empty_built(sref, view, top_n);
                };
                let info = SegmentInfo {
                    kind: seg.kind,
                    name: seg.name.clone(),
                    start_ms: seg.start_ms,
                    duration_ms: seg.duration_ms(self.now_ms),
                    success: seg.success,
                    live: seg.end_ms.is_none(),
                    instance: seg.visit,
                    pars_ms: None,
                };
                let rows = seg.rows(view);
                let breakdown = drill.map(|key| {
                    let (by_spell, by_target) = seg.breakdown(key, view);
                    Breakdown {
                        by_spell,
                        by_target,
                    }
                });
                Built::Ready(Box::new(self.snap(
                    sref,
                    Some(id),
                    view,
                    info,
                    rows,
                    top_n,
                    breakdown,
                    None,
                )))
            }
            Pos::Idx(i) => {
                let (Some(&id), Some(meta)) = (self.index_ids.get(i), self.index.get(i).cloned())
                else {
                    return self.empty_built(sref, view, top_n);
                };
                // The index is authoritative for the header: the lazily
                // loaded slice may lack its closing event, and the live
                // clock must never stretch history.
                let info = SegmentInfo {
                    kind: meta.kind,
                    name: meta.name.clone(),
                    start_ms: meta.start_ms,
                    duration_ms: meta.duration_ms,
                    success: meta.success,
                    live: false,
                    instance: meta.visit,
                    pars_ms: meta.pars_ms,
                };
                if self.touch_loaded(id) {
                    let (rows, breakdown) = {
                        // `touch_loaded` moved the hit to the back.
                        match self
                            .loaded
                            .last()
                            .and_then(|(_, meter)| meter.segments().first())
                        {
                            Some(seg) => {
                                let rows = seg.rows(view);
                                let breakdown = drill.map(|key| {
                                    let (by_spell, by_target) = seg.breakdown(key, view);
                                    Breakdown {
                                        by_spell,
                                        by_target,
                                    }
                                });
                                (rows, breakdown)
                            }
                            None => (Vec::new(), None),
                        }
                    };
                    Built::Ready(Box::new(self.snap(
                        sref,
                        Some(id),
                        view,
                        info,
                        rows,
                        top_n,
                        breakdown,
                        None,
                    )))
                } else {
                    let status = Some(format!("loading {}…", meta.name));
                    let snap =
                        self.snap(sref, Some(id), view, info, Vec::new(), top_n, None, status);
                    Built::Loading(Box::new(snap), id, meta)
                }
            }
            // R10: a closed visit's Overall — replay the visit's byte range,
            // then merge the members carrying its ordinal.
            Pos::IdxOverall(i) => {
                let (Some(&id), Some(meta)) = (
                    self.index_overall_ids.get(i),
                    self.index_overalls.get(i).cloned(),
                ) else {
                    return self.empty_built(sref, view, top_n);
                };
                let ordinal = meta.visit.unwrap_or(0);
                let info = SegmentInfo {
                    kind: SegmentKind::Overall,
                    name: meta.name.clone(),
                    start_ms: meta.start_ms,
                    duration_ms: meta.duration_ms,
                    success: meta.success,
                    live: false,
                    instance: meta.visit,
                    pars_ms: meta.pars_ms,
                };
                if self.touch_loaded(id) {
                    // `touch_loaded` moved the hit to the back.
                    let (rows, breakdown) = match self
                        .loaded
                        .last()
                        .and_then(|(_, meter)| meter.overall(ordinal))
                    {
                        Some(seg) => overall_rows(&seg, view, drill),
                        None => (Vec::new(), None),
                    };
                    Built::Ready(Box::new(self.snap(
                        sref,
                        Some(id),
                        view,
                        info,
                        rows,
                        top_n,
                        breakdown,
                        None,
                    )))
                } else {
                    let status = Some(format!("loading {}…", meta.name));
                    let snap =
                        self.snap(sref, Some(id), view, info, Vec::new(), top_n, None, status);
                    Built::Loading(Box::new(snap), id, meta)
                }
            }
            // R10: a visit with live members — merge them, plus the scanned
            // prefix when the daemon attached mid-visit.
            Pos::LiveOverall(ordinal) => {
                let Some(&id) = self.visit_ids.get(&ordinal) else {
                    return self.empty_built(sref, view, top_n);
                };
                let prefix_meta = self
                    .open_visit
                    .as_ref()
                    .filter(|m| m.visit == Some(ordinal))
                    .cloned();
                // The prefix (if any) must be resident before we can serve.
                if let Some(meta) = &prefix_meta
                    && !self.touch_loaded(id)
                {
                    let info = self.live_overall_info(ordinal, None);
                    let status = Some(format!("loading {}…", meta.name));
                    let snap =
                        self.snap(sref, Some(id), view, info, Vec::new(), top_n, None, status);
                    return Built::Loading(Box::new(snap), id, meta.clone());
                }
                let mut combined = self.meter.overall(ordinal);
                if prefix_meta.is_some()
                    && let Some((_, prefix)) = self.loaded.last().filter(|(i, _)| *i == id)
                    && let Some(prefix_seg) = prefix.overall(ordinal)
                {
                    match combined.as_mut() {
                        Some(seg) => seg.absorb(&prefix_seg),
                        None => combined = Some(prefix_seg),
                    }
                }
                let info = self.live_overall_info(ordinal, combined.as_ref());
                let (rows, breakdown) = match &combined {
                    Some(seg) => overall_rows(seg, view, drill),
                    None => (Vec::new(), None),
                };
                Built::Ready(Box::new(self.snap(
                    sref,
                    Some(id),
                    view,
                    info,
                    rows,
                    top_n,
                    breakdown,
                    None,
                )))
            }
        }
    }

    /// The "nothing to show" snapshot: no segment, no rows. Also the
    /// fallback when a resolved position no longer indexes its table.
    fn empty_built(&self, sref: SegmentRef, view: View, top_n: Option<u32>) -> Built {
        Built::Ready(Box::new(self.snap(
            sref,
            None,
            view,
            SegmentInfo {
                kind: SegmentKind::Trash,
                name: String::new(),
                start_ms: 0,
                duration_ms: 0,
                success: None,
                live: false,
                instance: None,
                pars_ms: None,
            },
            Vec::new(),
            top_n,
            None,
            None,
        )))
    }

    /// R10: header for a live visit's Overall. The list row and the meter
    /// header must agree, so the duration comes from the same clock.
    fn live_overall_info(
        &self,
        ordinal: u32,
        combined: Option<&wowdps_core::meter::Segment>,
    ) -> SegmentInfo {
        let v = self.meter.visits().get(ordinal as usize);
        SegmentInfo {
            kind: SegmentKind::Overall,
            name: combined.map_or_else(
                || v.map_or_else(String::new, |v| v.display_name()),
                |s| s.name.clone(),
            ),
            start_ms: v.map_or(0, |v| v.start_ms),
            duration_ms: combined.map_or_else(
                || self.live_overall_duration(ordinal),
                |s| s.duration_ms(self.now_ms),
            ),
            success: v.and_then(|v| v.verdict(self.now_ms)),
            live: v.is_some_and(|v| v.end_ms.is_none()),
            instance: Some(ordinal),
            pars_ms: v.and_then(|v| v.pars_ms),
        }
    }

    /// R11: the footer trash can — tombstone every closed, out-of-instance
    /// Trash segment. The live segment survives, and so does every visit
    /// member: keys and raids need them for their Σ overalls.
    pub fn discard_trash(&mut self) {
        for (m, id) in self.index.iter().zip(&self.index_ids) {
            if m.kind == SegmentKind::Trash && m.visit.is_none() {
                self.discarded.insert(*id);
            }
        }
        for (s, id) in self.meter.segments().iter().zip(&self.live_ids) {
            if s.kind == SegmentKind::Trash && s.visit.is_none() && s.end_ms.is_some() {
                self.discarded.insert(*id);
            }
        }
    }

    /// Test hook: how many historical meters are resident.
    #[doc(hidden)]
    pub fn resident(&self) -> usize {
        self.loaded.len()
    }

    #[allow(clippy::too_many_arguments)]
    fn snap(
        &self,
        segment: SegmentRef,
        id: Option<SegmentId>,
        view: View,
        info: SegmentInfo,
        rows: Vec<Row>,
        top_n: Option<u32>,
        breakdown: Option<Breakdown>,
        status: Option<String>,
    ) -> DaemonMsg {
        let total_rows = rows.len() as u32;
        let rows = match top_n {
            Some(n) => rows.into_iter().take(n as usize).collect(),
            None => rows,
        };
        DaemonMsg::Snapshot {
            seq: 0,
            segment,
            id,
            view,
            info,
            rows,
            total_rows,
            breakdown,
            // The full list length, Overall rows included: clients resolve
            // list positions against exactly this count.
            segment_count: self.list_entries_full().len() as u32,
            source: self.source_name.clone(),
            status: status.or_else(|| self.status.clone()),
        }
    }
}

/// Rows + optional drilldown from a merged Overall segment (R10).
fn overall_rows(
    seg: &wowdps_core::meter::Segment,
    view: View,
    drill: Option<&str>,
) -> (Vec<Row>, Option<Breakdown>) {
    let rows = seg.rows(view);
    let breakdown = drill.map(|key| {
        let (by_spell, by_target) = seg.breakdown(key, view);
        Breakdown {
            by_spell,
            by_target,
        }
    });
    (rows, breakdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daemon-wide bound that keeps N clients browsing N different
    /// segments from growing the process without limit.
    #[test]
    fn resident_history_is_capped_and_lru() {
        let mut e = Engine::new();
        for i in 0..(LOADED_CAP * 3) {
            e.install_loaded(SegmentId(i as u64), Meter::new());
            assert!(e.resident() <= LOADED_CAP);
        }
        assert_eq!(e.resident(), LOADED_CAP);
        // Re-installing an id it already holds must not double it.
        let last = SegmentId((LOADED_CAP * 3 - 1) as u64);
        e.install_loaded(last, Meter::new());
        assert_eq!(e.resident(), LOADED_CAP);
    }
}
