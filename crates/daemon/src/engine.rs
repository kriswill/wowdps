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
use wowdps_model::{Loadout, View};
use wowdps_proto::{Breakdown, CompareSide, DaemonMsg, ListEntry, LoadError, SegmentRef};

use crate::history::{ClosedFight, LogRef};

/// Parsed historical segments kept in memory, across *all* clients. Each is
/// one fight's per-actor hashmaps; the LRU bound is what keeps N clients
/// browsing N different segments from growing the daemon without limit.
pub const LOADED_CAP: usize = 16;

/// Something the hub should tell every session about.
pub enum EngineEvent {
    /// A new segment opened on fresh combat (not backlog replay).
    Opened(SegmentId),
    /// A segment or a visit closed on fresh combat — the history store's
    /// cue (`take_closed`). Backlog closes go through import instead.
    Closed(SegmentId),
}

/// What a cursor wants out of the segment it resolves to. Finding the segment
/// is identical for both; only the payload differs (R12).
enum Want<'a> {
    Meter {
        view: View,
        top_n: Option<u32>,
        drill: Option<&'a str>,
        /// v16: the drilled ability's by-spell key, for its own timeline.
        spell: Option<&'a str>,
    },
    Compare {
        a: &'a str,
        b: &'a str,
        /// v12: window the tables to `lo..hi` ms from the segment start.
        range: Option<(u32, u32)>,
        /// v18: the ability drill's by-spell key, applied to both sides.
        spell: Option<&'a str>,
    },
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

/// v19: what answering a `GetLoadout` came to. A one-shot has no snapshot to
/// placeholder with, so `Loading` carries only what the loader needs; the hub
/// parks the request and answers when the slice lands. Every unloadable case
/// is `Ready(None)` — the reply is defined to never error.
pub enum LoadoutBuilt {
    Ready(Option<Loadout>),
    Loading(SegmentId, SegmentMeta),
}

/// Where a `SegmentRef` points right now, resolved against the id tables.
enum Pos {
    None,
    Idx(usize),
    Live(usize),
    /// R10: a closed visit's Overall from the scan.
    IdxOverall(usize),
    /// R10: a visit served (at least partly) from the live meter.
    LiveOverall(u32),
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
    /// Visit ids whose `Closed` is waiting on the scanned prefix to load
    /// (`closed_needs_prefix`); the hub stores them when the load lands.
    pub history_pending: HashSet<SegmentId>,
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
    /// Parallel to `live_ids`: a `Closed` event was emitted for the segment.
    closed_seen: Vec<bool>,
    /// Visit ordinals whose Overall already emitted `Closed`.
    visits_closed: HashSet<u32>,
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
            history_pending: HashSet::new(),
            discarded: HashSet::new(),
            source_path: None,
            source_name: None,
            status: None,
            caught_up: false,
            closed_seen: Vec::new(),
            visits_closed: HashSet::new(),
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
                while self.closed_seen.len() < self.live_ids.len() {
                    self.closed_seen.push(false);
                }
                if self.caught_up {
                    self.emit_closed(out);
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
                        // R13: a noise segment never announces itself — it
                        // has no list row for the announcement to point at.
                        .is_some_and(|s| s.end_ms.is_none() && !s.noise)
                    && let Some(id) = self.live_ids.last()
                {
                    out.push(EngineEvent::Opened(*id));
                }
            }
            TailEvent::CaughtUp => {
                self.caught_up = true;
                // Whatever closed inside the replayed tail (a daemon restart
                // mid-session) is fresh to the store: emit it now, once.
                self.emit_closed(out);
            }
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
                self.history_pending.clear();
                self.status = None;
                self.caught_up = false;
                self.closed_seen.clear();
                self.visits_closed.clear();
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

    /// Emit `Closed` once per segment / visit whose `end_ms` turned `Some`.
    /// Noise segments are marked but never announced (no row, no record).
    fn emit_closed(&mut self, out: &mut Vec<EngineEvent>) {
        for (i, seg) in self.meter.segments().iter().enumerate() {
            if seg.end_ms.is_some() && self.closed_seen.get(i) == Some(&false) {
                if let Some(flag) = self.closed_seen.get_mut(i) {
                    *flag = true;
                }
                if !seg.noise
                    && let Some(&id) = self.live_ids.get(i)
                {
                    out.push(EngineEvent::Closed(id));
                }
            }
        }
        for (ord, v) in self.meter.visits().iter().enumerate() {
            let ord = ord as u32;
            if v.end_ms.is_some()
                && !self.visits_closed.contains(&ord)
                && let Some(&id) = self.visit_ids.get(&ord)
            {
                self.visits_closed.insert(ord);
                out.push(EngineEvent::Closed(id));
            }
        }
    }

    /// The closed fight behind a `Closed` event, cloned for the history
    /// thread: the live segment (with the visit it belongs to — a keyed
    /// run's bosses are not stored on their own), or a visit's Overall
    /// merged with its scanned prefix when the daemon attached mid-visit.
    /// `None` when that prefix is not resident — then `closed_needs_prefix`
    /// names the load the hub must make before asking again.
    pub fn take_closed(&self, id: SegmentId) -> Option<ClosedFight> {
        let path = self.source_path.clone()?;
        let log = LogRef { path };
        if let Some(i) = self.live_ids.iter().position(|&l| l == id) {
            let segment = self.meter.segments().get(i)?.clone();
            let visit = segment
                .visit
                .and_then(|ord| self.meter.visits().get(ord as usize))
                .cloned();
            return Some(ClosedFight {
                segment,
                visit,
                log,
                byte_range: None,
                aborted: false,
            });
        }
        let ord = *self.visit_ids.iter().find(|(_, v)| **v == id)?.0;
        let visit = self.meter.visits().get(ord as usize)?.clone();
        let mut segment = self.meter.overall(ord);
        if self
            .open_visit
            .as_ref()
            .is_some_and(|m| m.visit == Some(ord))
        {
            let (_, prefix) = self.loaded.iter().find(|(i, _)| *i == id)?;
            let prefix_seg = prefix.overall(ord)?;
            match segment.as_mut() {
                Some(seg) => seg.absorb(&prefix_seg),
                None => segment = Some(prefix_seg),
            }
        }
        let mut segment = segment?;
        segment.success = visit.verdict(segment.last_combat_ms());
        Some(ClosedFight {
            segment,
            visit: Some(visit),
            log,
            byte_range: None,
            aborted: false,
        })
    }

    /// The reason `take_closed` said `None` for a visit's Overall: the
    /// daemon attached mid-visit and the scanned prefix is not resident
    /// (nobody watched the Σ, or the LRU evicted it). Returns the prefix
    /// meta to load; once `install_loaded` has it, `take_closed` serves.
    pub fn closed_needs_prefix(&self, id: SegmentId) -> Option<SegmentMeta> {
        let ord = *self.visit_ids.iter().find(|(_, v)| **v == id)?.0;
        let meta = self.open_visit.as_ref().filter(|m| m.visit == Some(ord))?;
        (!self.loaded.iter().any(|(i, _)| *i == id)).then(|| meta.clone())
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
                        arena: m.arena,
                        encounter: m.encounter,
                    },
                )
            });
        let live = self
            .meter
            .segments()
            .iter()
            .zip(&self.live_ids)
            // R13: a noise segment (post-match arena tail) never surfaces,
            // not even while live — R11's live exception doesn't apply.
            .filter(|(s, id)| {
                ((s.end_ms.is_none() && !s.noise) || s.counts()) && !discarded.contains(id)
            })
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
                        arena: s.arena,
                        encounter: s.encounter,
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
                        arena: false,
                        encounter: None,
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
                    arena: false,
                    encounter: None,
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
            // Re-read each time the list is rebuilt (rarely): the header may
            // still be half a line when the file appears.
            log_id: self
                .source_path
                .as_deref()
                .map(crate::history::LogFacts::read)
                .filter(|f| f.complete)
                .map(|f| f.id),
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
    /// A meter cursor: rows for a view, optionally drilled.
    pub fn build_segment(
        &mut self,
        sref: SegmentRef,
        view: View,
        top_n: Option<u32>,
        drill: Option<&str>,
        spell: Option<&str>,
    ) -> Built {
        self.build(
            sref,
            &Want::Meter {
                view,
                top_n,
                drill,
                spell,
            },
        )
    }

    /// R12: a comparison cursor — the same segment resolution, a different
    /// payload. Everything about *finding* the segment (live, lazily loaded,
    /// a visit's merged Overall, rotated away) is shared with the meter path,
    /// so a comparison can be opened on any segment a meter can.
    pub fn build_compare(
        &mut self,
        sref: SegmentRef,
        a: &str,
        b: &str,
        range: Option<(u32, u32)>,
        spell: Option<&str>,
    ) -> Built {
        self.build(sref, &Want::Compare { a, b, range, spell })
    }

    /// v19: one player's COMBATANT_INFO loadout for one segment — the same
    /// segment resolution as the snapshot paths, a much smaller payload.
    /// The answer is always the SEGMENT'S OWN map — "the latest line at or
    /// before it" — never the live meter's whole-log view, which could hand
    /// a warm query a build first seen in a LATER segment (a respec pinned to
    /// the wrong fight, and a different answer than the same id's lazy replay
    /// after a restart). A cold historical segment returns `Loading` for the
    /// hub to park the request behind the loader.
    pub fn loadout(&mut self, sref: SegmentRef, guid: &str) -> LoadoutBuilt {
        let pos = match self.resolve(sref) {
            Ok(pos) => pos,
            // Rotated/NotFound: a one-shot never errors, it answers None.
            Err(_) => return LoadoutBuilt::Ready(None),
        };
        match pos {
            Pos::None => LoadoutBuilt::Ready(None),
            Pos::Live(i) => LoadoutBuilt::Ready(
                self.meter
                    .segments()
                    .get(i)
                    .and_then(|s| s.loadout(guid))
                    .cloned(),
            ),
            // The LIVE visit: nothing exists after it, so the meter-level map
            // IS "latest at or before" — and matches the Overall merge's
            // later-member-wins rule.
            Pos::LiveOverall(_) => LoadoutBuilt::Ready(self.meter.loadout(guid).cloned()),
            Pos::Idx(i) => {
                let Some(&id) = self.index_ids.get(i) else {
                    return LoadoutBuilt::Ready(None);
                };
                if self.touch_loaded(id) {
                    LoadoutBuilt::Ready(
                        self.resident_meter()
                            .and_then(|m| m.segments().first())
                            .and_then(|s| s.loadout(guid))
                            .cloned(),
                    )
                } else if let Some(meta) = self.index.get(i) {
                    LoadoutBuilt::Loading(id, meta.clone())
                } else {
                    LoadoutBuilt::Ready(None)
                }
            }
            Pos::IdxOverall(i) => {
                let Some(&id) = self.index_overall_ids.get(i) else {
                    return LoadoutBuilt::Ready(None);
                };
                if self.touch_loaded(id) {
                    // Meter-level on the loaded visit replay = the members'
                    // merged view, the Overall's own semantics.
                    LoadoutBuilt::Ready(
                        self.resident_meter().and_then(|m| m.loadout(guid)).cloned(),
                    )
                } else if let Some(meta) = self.index_overalls.get(i) {
                    LoadoutBuilt::Loading(id, meta.clone())
                } else {
                    LoadoutBuilt::Ready(None)
                }
            }
        }
    }

    /// The meter a `touch_loaded` hit just moved to the LRU's back. Every
    /// warm path — snapshot and loadout alike — reads residency through this
    /// pair, so the discipline cannot diverge between them.
    fn resident_meter(&self) -> Option<&Meter> {
        self.loaded.last().map(|(_, m)| m)
    }

    /// Resolve a `SegmentRef` against the id tables — shared by the snapshot
    /// path and the loadout path so the two can never disagree on identity.
    fn resolve(&self, sref: SegmentRef) -> Result<Pos, (SegmentId, LoadError)> {
        Ok(match sref {
            SegmentRef::Live => {
                // R13: the live cursor skips noise segments — after an arena
                // match ends, "live" stays on the finished match (its LOSS/WIN
                // tag showing) instead of following the leftover pet tail.
                if let Some(i) = self.meter.segments().iter().rposition(|s| !s.noise) {
                    Pos::Live(i)
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
                    return Err((id, LoadError::Rotated));
                } else {
                    return Err((id, LoadError::NotFound));
                }
            }
        })
    }

    fn build(&mut self, sref: SegmentRef, want: &Want) -> Built {
        let (view, top_n) = match want {
            Want::Meter { view, top_n, .. } => (*view, *top_n),
            // A comparison is always over damage; the view is only carried
            // here so the shared resolution below can keep its shape.
            Want::Compare { .. } => (View::Damage, None),
        };
        let pos = match self.resolve(sref) {
            Ok(pos) => pos,
            Err((id, error)) => return Built::Failed(id, error),
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
                    arena: seg.arena,
                    encounter: seg.encounter,
                };
                let msg = self.render(sref, Some(id), info, want, Some(seg), None);
                Built::Ready(Box::new(msg))
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
                    arena: meta.arena,
                    encounter: meta.encounter,
                };
                if self.touch_loaded(id) {
                    let seg = self
                        .resident_meter()
                        .and_then(|meter| meter.segments().first());
                    let msg = self.render(sref, Some(id), info, want, seg, None);
                    Built::Ready(Box::new(msg))
                } else {
                    let status = Some(wowdps_proto::loading_status(&meta.name));
                    let snap = self.render(sref, Some(id), info, want, None, status);
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
                    arena: false,
                    encounter: None,
                };
                if self.touch_loaded(id) {
                    let merged = self
                        .resident_meter()
                        .and_then(|meter| meter.overall(ordinal));
                    let msg = self.render(sref, Some(id), info, want, merged.as_ref(), None);
                    Built::Ready(Box::new(msg))
                } else {
                    let status = Some(wowdps_proto::loading_status(&meta.name));
                    let snap = self.render(sref, Some(id), info, want, None, status);
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
                    let status = Some(wowdps_proto::loading_status(&meta.name));
                    let snap = self.render(sref, Some(id), info, want, None, status);
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
                let msg = self.render(sref, Some(id), info, want, combined.as_ref(), None);
                Built::Ready(Box::new(msg))
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
                arena: false,
                encounter: None,
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
            arena: false,
            encounter: None,
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

    /// Turn a resolved segment (or the absence of one, while a slice loads)
    /// into the message the cursor asked for.
    fn render(
        &self,
        sref: SegmentRef,
        id: Option<SegmentId>,
        info: SegmentInfo,
        want: &Want,
        seg: Option<&wowdps_core::meter::Segment>,
        status: Option<String>,
    ) -> DaemonMsg {
        match want {
            Want::Meter {
                view,
                top_n,
                drill,
                spell,
            } => {
                let rows = seg.map(|s| s.rows(*view)).unwrap_or_default();
                let breakdown = seg.zip(*drill).map(|(s, key)| {
                    let (by_spell, by_target) = s.breakdown(key, *view);
                    Breakdown {
                        by_spell,
                        by_target,
                        // v14: the drilled view's own curve — damage or
                        // effective healing; the count views have no graph.
                        timeline: match *view {
                            View::Damage => Some(s.timeline(key)),
                            View::Healing => Some(s.heal_timeline(key)),
                            _ => None,
                        },
                        // v16: the drilled ability's own curve, over the
                        // ghosted player line. Damage only — the sparse
                        // per-spell series records nothing else.
                        spell_timeline: (*view == View::Damage)
                            .then_some(*spell)
                            .flatten()
                            .map(|sk| s.spell_timeline(key, sk)),
                        // v17: who the ability landed on, for any view.
                        spell_targets: spell.map(|sk| s.spell_targets(key, sk, *view)),
                    }
                });
                self.snap(sref, id, *view, info, rows, *top_n, breakdown, status)
            }
            Want::Compare { a, b, range, spell } => DaemonMsg::CompareSnapshot {
                seq: 0,
                segment: sref,
                id,
                info,
                a: Box::new(compare_side(seg, a, *range, *spell)),
                b: Box::new(compare_side(seg, b, *range, *spell)),
                range: *range,
                source: self.source_name.clone(),
                status: status.or_else(|| self.status.clone()),
            },
        }
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

/// R12: one player's half of a comparison. A player who isn't in the segment
/// (picked on a different fight, or simply idle) yields an empty side rather
/// than an error — the pane draws a zeroed column and the pair survives.
fn compare_side(
    seg: Option<&wowdps_core::meter::Segment>,
    guid: &str,
    range: Option<(u32, u32)>,
    spell: Option<&str>,
) -> CompareSide {
    let Some(seg) = seg else {
        return CompareSide {
            guid: guid.to_string(),
            ..Default::default()
        };
    };
    // v12: a windowed comparison answers from the segment's sparse per-spell
    // series — total and tables wear the window's own numbers; the timeline
    // stays whole (the graph zoom is the client's slice).
    let (total, spells) = match range {
        Some((lo, hi)) => {
            let (mut total, spells) = seg.compare_spells(guid, Some((lo as i64, hi as i64)));
            total.key = guid.to_string();
            (total, spells)
        }
        None => {
            let total = seg
                .rows(View::Damage)
                .into_iter()
                .find(|r| r.key == guid)
                .unwrap_or_else(|| Row {
                    key: guid.to_string(),
                    ..Row::default()
                });
            let (spells, _) = seg.breakdown(guid, View::Damage);
            (total, spells)
        }
    };
    CompareSide {
        guid: guid.to_string(),
        total,
        spells,
        timeline: seg.timeline(guid),
        // v18: the drilled ability's curve for THIS side; empty buckets mean
        // this player never cast it, and the client draws no focus then.
        spell_timeline: spell
            .map(|sk| seg.spell_timeline(guid, sk))
            .filter(|t| !t.buckets.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal advanced-format damage line the parser accepts (same shape
    /// as the ipc suite's helper). 900 damage from Player-1-A to a boss.
    fn hit(min: u32, sec: u32) -> String {
        format!(
            "7/27/2026 21:{min:02}:{sec:02}.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n"
        )
    }

    fn feed(e: &mut Engine, lines: Vec<String>) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        e.on_tail(TailEvent::Lines(lines), &mut out);
        out
    }

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

    /// The liveness verdict is observation + the game-process signal, never
    /// file mtime: the game flushes the log in multi-minute bursts, so a
    /// stale-looking file with the game running must still read as live —
    /// and backlog replay (lines before CaughtUp) must never count as
    /// "lines arriving now".
    #[test]
    fn liveness_needs_catchup_an_open_segment_and_a_signal() {
        let mut e = Engine::new();
        // Nothing at all: not live, no matter what the game watcher says.
        assert!(!e.live_now(true), "empty engine is never live");

        // Backlog opens a segment, but we have not caught up yet: replaying
        // history is not combat happening now.
        feed(&mut e, vec![hit(0, 0)]);
        assert!(!e.live_now(true), "backlog replay is not live");

        // CaughtUp with an open segment: the backlog lines arrived *before*
        // catch-up, so the fresh-lines signal is absent. Only the
        // game-process signal makes this live — exactly the flush-burst
        // scenario where mtime would lie.
        e.on_tail(TailEvent::CaughtUp, &mut Vec::new());
        assert!(
            !e.live_now(false),
            "no fresh lines and no game process: a stale file's open tail is not a fight"
        );
        assert!(
            e.live_now(true),
            "the game-process signal alone keeps an open segment live across flush gaps"
        );
    }

    /// The other half of the verdict: post-catch-up lines are the
    /// observation signal, so replays and tests read as live without any
    /// game process at all.
    #[test]
    fn fresh_lines_after_catchup_are_live_without_the_game() {
        let mut e = Engine::new();
        e.on_tail(TailEvent::CaughtUp, &mut Vec::new());
        feed(&mut e, vec![hit(0, 0)]);
        assert!(e.live_now(false), "fresh combat needs no game process");
    }

    /// Ids are daemon-lifetime monotonic and never reused: after a rotation,
    /// every new id is strictly greater than every old one, and a stale id
    /// resolves to Rotated (its file is gone), never to another file's fight.
    /// An id the daemon never issued is NotFound, not Rotated.
    #[test]
    fn rotation_retires_ids_and_reports_rotated_vs_notfound() {
        let mut e = Engine::new();
        e.on_tail(
            TailEvent::Switched(PathBuf::from("/tmp/a.txt")),
            &mut Vec::new(),
        );
        feed(&mut e, vec![hit(0, 0)]);
        let old_ids = e.list_ids();
        assert!(!old_ids.is_empty(), "the open segment gets an id");
        let old_max = old_ids.iter().map(|i| i.0).max().unwrap();

        e.on_tail(
            TailEvent::Switched(PathBuf::from("/tmp/b.txt")),
            &mut Vec::new(),
        );
        feed(&mut e, vec![hit(0, 0)]);
        let new_ids = e.list_ids();
        assert!(
            new_ids.iter().all(|i| i.0 > old_max),
            "ids never reused across rotation: {new_ids:?} vs max {old_max}"
        );

        // The old file's id: issued, but below the new file's floor.
        let old_id = SegmentId(old_max);
        match e.build_segment(SegmentRef::Id(old_id), View::Damage, None, None, None) {
            Built::Failed(id, LoadError::Rotated) => assert_eq!(id, old_id),
            _ => panic!("a rotated-away id must fail with Rotated"),
        }
        // An id from the future: never issued, so NotFound.
        let bogus = SegmentId(1_000_000);
        match e.build_segment(SegmentRef::Id(bogus), View::Damage, None, None, None) {
            Built::Failed(id, LoadError::NotFound) => assert_eq!(id, bogus),
            _ => panic!("a never-issued id must fail with NotFound"),
        }
    }

    /// `Opened` announces fresh combat only: nothing during backlog replay,
    /// one event when a new segment opens after catch-up, and no re-announce
    /// while the same pull continues.
    #[test]
    fn opened_fires_once_per_fresh_segment_and_never_for_backlog() {
        let mut e = Engine::new();
        // Backlog: a segment opens, silently.
        let ev = feed(&mut e, vec![hit(0, 0)]);
        assert!(ev.is_empty(), "backlog segments never announce");
        e.on_tail(TailEvent::CaughtUp, &mut Vec::new());

        // A jump past the trash gap closes the old segment and opens a new
        // one — that new, still-open segment is the announcement.
        let ev = feed(&mut e, vec![hit(10, 0)]);
        let opened: Vec<_> = ev
            .iter()
            .filter_map(|e| match e {
                EngineEvent::Opened(id) => Some(*id),
                EngineEvent::Closed(_) => None,
            })
            .collect();
        assert_eq!(opened.len(), 1, "exactly one announcement per new pull");
        assert_eq!(
            Some(&opened[0]),
            e.list_ids().last(),
            "the announced id is the newest live segment"
        );

        // The same pull continuing must not re-announce.
        let ev = feed(&mut e, vec![hit(10, 5)]);
        assert!(ev.is_empty(), "a continuing segment is not a new one");
    }

    /// R11: the trash can tombstones closed, out-of-instance Trash only —
    /// the live segment always survives, so the meter keeps showing the
    /// fight in progress.
    #[test]
    fn discard_trash_drops_closed_trash_but_keeps_the_live_segment() {
        let mut e = Engine::new();
        // Two hits a trash-gap apart: the first segment closes, the second
        // stays open.
        feed(&mut e, vec![hit(0, 0), hit(10, 0)]);
        assert_eq!(e.list_rows().len(), 2, "one closed + one live");

        e.discard_trash();
        let rows = e.list_rows();
        assert_eq!(rows.len(), 1, "only the live segment survives the can");
        assert!(rows[0].live, "and it is the live one");
    }

    /// An empty engine still answers a Live cursor: an empty snapshot, not
    /// an error and not a hang — a client can watch before any log exists.
    #[test]
    fn an_empty_engine_serves_an_empty_live_snapshot() {
        let mut e = Engine::new();
        match e.build_segment(SegmentRef::Live, View::Damage, None, None, None) {
            Built::Ready(msg) => match *msg {
                DaemonMsg::Snapshot { id, rows, .. } => {
                    assert_eq!(id, None, "no segment resolved");
                    assert!(rows.is_empty(), "and no rows invented");
                }
                other => panic!("expected a Snapshot, got {other:?}"),
            },
            _ => panic!("an empty engine must answer Ready, not Loading/Failed"),
        }
    }
}
