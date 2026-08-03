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
            live_ids: Vec::new(),
            next_id: 0,
            first_id_of_file: 0,
            loaded: Vec::new(),
            loading: HashSet::new(),
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
                self.seen_segments = self.segment_count();
            }
            TailEvent::Waiting => self.source_name = None,
            TailEvent::Error(msg) => self.status = Some(msg),
        }
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

    /// Ids for the combined list, position-aligned with `list_rows`.
    pub fn list_ids(&self) -> Vec<SegmentId> {
        self.index_ids
            .iter()
            .chain(self.live_ids.iter())
            .copied()
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
                } else if id.0 < self.first_id_of_file && id.0 < self.next_id {
                    return Built::Failed(id, LoadError::Rotated);
                } else {
                    return Built::Failed(id, LoadError::NotFound);
                }
            }
        };

        match pos {
            Pos::None => Built::Ready(Box::new(self.snap(
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
                },
                Vec::new(),
                top_n,
                None,
                None,
            ))),
            Pos::Live(i) => {
                let id = self.live_ids[i];
                let seg = &self.meter.segments()[i];
                let info = SegmentInfo {
                    kind: seg.kind,
                    name: seg.name.clone(),
                    start_ms: seg.start_ms,
                    duration_ms: seg.duration_ms(self.now_ms),
                    success: seg.success,
                    live: seg.end_ms.is_none(),
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
                let id = self.index_ids[i];
                let meta = self.index[i].clone();
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
                };
                if self.touch_loaded(id) {
                    let (rows, breakdown) = {
                        let meter = &self.loaded.last().expect("just touched").1;
                        match meter.segments().first() {
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
            segment_count: self.segment_count() as u32,
            source: self.source_name.clone(),
            status: status.or_else(|| self.status.clone()),
        }
    }
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
