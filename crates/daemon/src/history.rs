//! The history store (roadmap item 1, `docs/spec-history-store.md`): fights
//! persist across sessions as per-fight JSON documents under
//! `$XDG_DATA_HOME/wowdps/history/v1/`, written by this thread, indexed in
//! memory from their ~400 B cards, and never touched by the hub beyond one
//! `Segment` clone and a `try_send`.
//!
//! Non-negotiables (spec §2), each with its home here:
//! - summaries, never events — `extract` derives everything from a
//!   `Segment` the way a snapshot would;
//! - stdlib only — `proto::history` is the codec, `write_atomic` the durability;
//! - the files are the truth — `Store::open` rebuilds the index from
//!   `fights/*.json` and nothing else is persisted;
//! - decode never panics — a torn or foreign file is skipped and counted
//!   once in `Status`;
//! - a live meter is never delayed — `HistoryLink::send` is a bounded
//!   `try_send`; a full channel drops the write and counts it.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use wowdps_core::index::{self, SegmentMeta};
use wowdps_core::meter::{Meter, Segment, SegmentKind, Visit};
use wowdps_core::model::{SegmentId, View};
use wowdps_core::parser::tz_offset_min;
use wowdps_core::tail::{SourceSpec, newest_log};
use wowdps_proto::history::{
    CardPlayer, FightCard, FightDetails, FightKind, FightRows, HISTORY_SCHEMA, KeyInfo,
    PlayerDetail, Recap, StoredLoadout, content_id, fight_id, loadout_hash, log_id,
};
use wowdps_proto::json;
use wowdps_proto::msg::HistoryStatus;
use wowdps_proto::{
    Breakdown, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery, Night, StoredFight, TrendBucket,
    TrendPoint,
};

use crate::cache::{IndexCache, write_atomic};
use crate::hub::HubMsg;
use crate::loader::{LoadReply, LoadReq};

/// Bound of the hub → history channel. A night's worth of pulls is a few
/// dozen; 64 in flight means the thread is wedged, and dropping (counted)
/// beats stalling the meter.
pub const QUEUE: usize = 64;

/// `history_*` keys of `~/.config/wowdps/config.toml`, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryOptions {
    pub dir: PathBuf,
    pub store_trash: bool,
    pub keep_per_encounter: usize,
    pub keep_details_per_encounter: usize,
    /// "Name-Realm" strings that are "me" (spec §9); empty = infer.
    pub characters: Vec<String>,
    /// The index-checkpoint cache, so the start-up sweep of old logs costs
    /// a tail rescan, not a full one.
    pub cache_dir: Option<PathBuf>,
}

impl HistoryOptions {
    /// `$XDG_DATA_HOME/wowdps/history/v1`, else `~/.local/share/...`.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(base.join("wowdps/history/v1"))
    }
}

/// Which log a fight came out of. The identity (`proto::history::log_id`)
/// is resolved lazily on the history thread — the daemon retargets to a new
/// log the moment it appears, when it may hold half a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRef {
    pub path: PathBuf,
}

/// A fight the hub saw close, cloned out of the engine for this thread.
#[derive(Debug, Clone)]
pub struct ClosedFight {
    pub segment: Segment,
    /// The visit an Overall aggregates (its key facts live here).
    pub visit: Option<Visit>,
    pub log: LogRef,
    /// Provenance when the index had it.
    pub byte_range: Option<(u64, u64)>,
    /// Open at the end of a finished log: listed, never a pull.
    pub aborted: bool,
}

/// One historical segment the import path asked the loader pool to parse.
#[derive(Debug, Clone)]
pub struct ImportJob {
    pub log: LogRef,
    pub meta: SegmentMeta,
    pub aborted: bool,
}

pub enum HistoryReq {
    /// A fight closed on the live meter.
    Store(Box<ClosedFight>),
    /// The tailed log's index: enqueue whatever it holds that the store
    /// lacks (backlog goes through import, never through `Store`).
    Index {
        log: LogRef,
        segments: Vec<SegmentMeta>,
        overalls: Vec<SegmentMeta>,
    },
    /// Scan a log or a directory of logs and import what is missing
    /// (start-up, `wowdps history import`).
    Sweep(PathBuf),
    /// The loader pool finished an import job.
    Loaded {
        job: Box<ImportJob>,
        result: Result<Box<Meter>, String>,
    },
    /// v20: a session's one-shot, answered through `HubMsg::History`.
    Query {
        session: u64,
        req_id: u32,
        query: HistoryQuery,
    },
    Fight {
        session: u64,
        req_id: u32,
        fight_id: String,
        view: View,
        drill: Option<String>,
    },
    Pin {
        session: u64,
        req_id: u32,
        fight_id: String,
        pinned: bool,
    },
    ImportLog {
        session: u64,
        req_id: u32,
        path: PathBuf,
    },
}

/// The hub's handle: a bounded sender plus the status the hub reads
/// synchronously for `Status`. Cloneable so the loader pool can answer
/// import jobs straight back to the thread.
#[derive(Clone)]
pub struct HistoryLink {
    tx: Option<SyncSender<HistoryReq>>,
    status: Arc<Mutex<HistoryStatus>>,
}

impl HistoryLink {
    /// No store: every send is a no-op and `Status` says why.
    pub fn disabled(reason: &str) -> Self {
        Self {
            tx: None,
            status: Arc::new(Mutex::new(HistoryStatus {
                enabled: false,
                error: Some(reason.to_string()),
                ..HistoryStatus::default()
            })),
        }
    }

    /// Never blocks: a full queue drops the request and counts it.
    pub fn send(&self, req: HistoryReq) {
        let Some(tx) = &self.tx else { return };
        match tx.try_send(req) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                if let Ok(mut s) = self.status.lock() {
                    s.dropped = s.dropped.saturating_add(1);
                }
            }
        }
    }

    /// The loader pool's reply path: blocks until the thread takes it. A
    /// lost `Loaded` would leave the import queue wedged forever (the reply
    /// is the only thing that clears `inflight`), so it never rides the
    /// lossy `try_send`; the pool has a worker to spare and the history
    /// thread always drains.
    pub fn reply(&self, req: HistoryReq) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(req);
        }
    }

    pub fn status(&self) -> HistoryStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    /// A link over a channel of `capacity` with no thread behind it, so a
    /// test can fill the queue and watch `send` drop and count.
    #[doc(hidden)]
    pub fn bounded(capacity: usize) -> (Self, Receiver<HistoryReq>) {
        let (tx, rx) = sync_channel(capacity);
        let link = Self {
            tx: Some(tx),
            status: Arc::new(Mutex::new(HistoryStatus {
                enabled: true,
                ..HistoryStatus::default()
            })),
        };
        (link, rx)
    }
}

/// Start the history thread over a directory. `sweep` names what to import
/// on start (the daemon's own source: its file, or every log in its dir).
pub fn spawn(
    opts: HistoryOptions,
    loader: Sender<LoadReq>,
    hub: Sender<HubMsg>,
    sweep: Option<&SourceSpec>,
) -> HistoryLink {
    let (tx, rx) = sync_channel::<HistoryReq>(QUEUE);
    let status = Arc::new(Mutex::new(HistoryStatus {
        enabled: true,
        ..HistoryStatus::default()
    }));
    let link = HistoryLink {
        tx: Some(tx),
        status: Arc::clone(&status),
    };
    let sweep_root = sweep.map(|s| match s {
        SourceSpec::File(p) | SourceSpec::Dir(p) => p.clone(),
    });
    let source = sweep.cloned();
    let reply = link.clone();
    thread::spawn(move || {
        let cache = opts.cache_dir.clone().map(IndexCache::new);
        let store = Store::open(DirBackend::new(opts.dir.clone()), Retention::from(&opts));
        let mut worker = Worker {
            store,
            loader,
            reply,
            hub,
            cache,
            queue: VecDeque::new(),
            queued: HashSet::new(),
            inflight: false,
            logs: HashMap::new(),
            source,
        };
        worker.publish(&status);
        if let Some(root) = sweep_root {
            worker.sweep(&root);
            worker.publish(&status);
        }
        run(rx, worker, status);
    });
    link
}

fn run(rx: Receiver<HistoryReq>, mut w: Worker<DirBackend>, status: Arc<Mutex<HistoryStatus>>) {
    while let Ok(req) = rx.recv() {
        w.handle(req);
        w.publish(&status);
    }
}

/// The thread's state: the store plus the import queue.
struct Worker<B: Backend> {
    store: Store<B>,
    loader: Sender<LoadReq>,
    reply: HistoryLink,
    /// Replies to sessions and the `HistoryChanged` broadcast go back
    /// through the hub, which owns the session table.
    hub: Sender<HubMsg>,
    cache: Option<IndexCache>,
    queue: VecDeque<ImportJob>,
    /// Fight ids queued or in flight, so a sweep and an `Index` of the same
    /// log never parse a segment twice.
    queued: HashSet<String>,
    /// One import load outstanding at a time: the pool has two workers and
    /// a watching client must always find one free.
    inflight: bool,
    /// Per-log identity, resolved once.
    logs: HashMap<PathBuf, LogFacts>,
    /// The daemon's own source: whichever log it tails is live, and only
    /// that log's open tail and open visit are left to the engine.
    source: Option<SourceSpec>,
}

impl<B: Backend> Worker<B> {
    fn handle(&mut self, req: HistoryReq) {
        match req {
            HistoryReq::Store(fight) => {
                let facts = self.facts(&fight.log.path);
                if let Some(id) = self.store.store(&fight, facts) {
                    self.changed(id);
                }
            }
            HistoryReq::Query {
                session,
                req_id,
                query,
            } => {
                let answer = self.store.answer(&query);
                self.reply_to(session, DaemonMsg::History { req_id, answer });
            }
            HistoryReq::Fight {
                session,
                req_id,
                fight_id,
                view,
                drill,
            } => {
                let fight = self.store.stored_fight(&fight_id, view, drill.as_deref());
                self.reply_to(session, DaemonMsg::Fight { req_id, fight });
            }
            HistoryReq::Pin {
                session,
                req_id,
                fight_id,
                pinned,
            } => {
                let pinned = self.store.pin(&fight_id, pinned) && pinned;
                self.reply_to(
                    session,
                    DaemonMsg::History {
                        req_id,
                        answer: HistoryAnswer::Pinned {
                            fight_id: fight_id.clone(),
                            pinned,
                        },
                    },
                );
                self.changed(fight_id);
            }
            HistoryReq::ImportLog {
                session,
                req_id,
                path,
            } => {
                let before = self.queue.len();
                self.sweep(&path);
                let queued = (self.queue.len() + usize::from(self.inflight)).saturating_sub(before);
                self.reply_to(
                    session,
                    DaemonMsg::History {
                        req_id,
                        answer: HistoryAnswer::Imported {
                            queued: queued as u32,
                        },
                    },
                );
            }
            HistoryReq::Index {
                log,
                segments,
                overalls,
            } => {
                self.enqueue_metas(&log, segments, overalls, None);
                self.dispatch();
            }
            HistoryReq::Sweep(root) => {
                self.sweep(&root);
            }
            HistoryReq::Loaded { job, result } => {
                self.inflight = false;
                let id = self.facts(&job.log.path).id;
                self.queued.remove(&fight_id(id, job.meta.start_ms));
                match result {
                    Ok(meter) => {
                        if let Some(fight) = fight_from_import(&job, &meter) {
                            let facts = self.facts(&fight.log.path);
                            if let Some(id) = self.store.store(&fight, facts) {
                                self.changed(id);
                            }
                        }
                    }
                    Err(e) => self.store.last_error = Some(e),
                }
                self.dispatch();
            }
        }
    }

    fn reply_to(&self, session: u64, msg: DaemonMsg) {
        let _ = self.hub.send(HubMsg::History {
            session,
            msg: Box::new(msg),
        });
    }

    fn changed(&self, fight_id: String) {
        let _ = self.hub.send(HubMsg::HistoryChanged { fight_id });
    }

    fn publish(&self, status: &Arc<Mutex<HistoryStatus>>) {
        if let Ok(mut s) = status.lock() {
            let mine = self.store.status();
            s.enabled = true;
            s.fights = mine.fights;
            s.owner_inferred = mine.owner_inferred;
            s.error = mine.error;
            s.importing = (self.queue.len() + usize::from(self.inflight)) as u32;
        }
    }

    fn facts(&mut self, path: &Path) -> LogFacts {
        if let Some(f) = self.logs.get(path) {
            return *f;
        }
        let f = LogFacts::read(path);
        // Only a real identity is worth remembering; a provisional one is
        // re-read on every use until the header lands.
        if f.complete {
            self.logs.insert(path.to_path_buf(), f);
        }
        f
    }

    /// Every closed meta of a log that the store lacks becomes an import
    /// job; `open` (a segment still open at the end of a finished log) an
    /// aborted one.
    fn enqueue_metas(
        &mut self,
        log: &LogRef,
        segments: Vec<SegmentMeta>,
        overalls: Vec<SegmentMeta>,
        open: Option<SegmentMeta>,
    ) {
        let facts = self.facts(&log.path);
        // Keyed visits: their Overall carries the par timers or a "+N"
        // display name (`Visit::display_name`).
        let keyed: HashSet<u32> = overalls
            .iter()
            .filter(|m| m.pars_ms.is_some() || looks_keyed(&m.name))
            .filter_map(|m| m.visit)
            .collect();
        let closed = segments
            .into_iter()
            .chain(overalls)
            .map(|m| (m, false))
            .chain(open.into_iter().map(|m| (m, true)));
        for (meta, aborted) in closed {
            if !self.store.wants_meta(&meta, &keyed) {
                continue;
            }
            let id = fight_id(facts.id, meta.start_ms);
            if self.store.has(&id) || self.queued.contains(&id) {
                continue;
            }
            self.queued.insert(id);
            self.queue.push_back(ImportJob {
                log: log.clone(),
                meta,
                aborted,
            });
        }
    }

    fn dispatch(&mut self) {
        if self.inflight {
            return;
        }
        let Some(job) = self.queue.pop_front() else {
            return;
        };
        let req = LoadReq {
            // Never resolved against the engine's tables: the reply carries
            // the job, not the id.
            id: SegmentId(u64::MAX),
            path: job.log.path.clone(),
            meta: job.meta.clone(),
            reply: LoadReply::History {
                link: self.reply.clone(),
                job: Box::new(job),
            },
        };
        if self.loader.send(req).is_ok() {
            self.inflight = true;
        }
    }

    /// Scan one log, or every `WoWCombatLog*.txt` in a directory newest
    /// first, and queue what the store lacks.
    fn sweep(&mut self, root: &Path) {
        let files: Vec<PathBuf> = if root.is_dir() {
            let mut all: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(root)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.starts_with("WoWCombatLog") && n.ends_with(".txt")
                })
                .filter_map(|e| {
                    let mtime = e.metadata().ok()?.modified().ok()?;
                    Some((mtime, e.path()))
                })
                .collect();
            all.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));
            all.into_iter().map(|(_, p)| p).collect()
        } else {
            vec![root.to_path_buf()]
        };
        // The tailed log — the daemon's file, or the newest of its directory
        // — is the one whose open tail and open visit are live. A file
        // handed to `wowdps history import` is an older session unless it
        // IS that log, so its open visit (the night's last key) is imported.
        let newest = match &self.source {
            Some(SourceSpec::File(p)) => Some(p.clone()),
            Some(SourceSpec::Dir(d)) => newest_log(d),
            None if root.is_dir() => newest_log(root),
            None => Some(root.to_path_buf()),
        };
        for path in files {
            let Ok(mut file) = std::fs::File::open(&path) else {
                continue;
            };
            let idx = match &self.cache {
                Some(cache) => cache.scan_file(&path, &mut file),
                None => index::scan(&mut file),
            };
            // The newest log is the tailed one: its open tail is live, not
            // aborted. Anything still open in an older log never closes —
            // including its last VISIT: zoning out only suspends a visit
            // (R10), so the night's last key, or the raid itself, is still
            // open at EOF and its Σ exists only as `open_visit`. A keyed run
            // whose END fired is a finished run (not aborted); a key without
            // one is; a plain visit's Σ merges only closed members and is
            // stored as is.
            let (open, overalls) = if Some(&path) == newest.as_ref() {
                (None, idx.overalls)
            } else {
                let mut overalls = idx.overalls;
                if let Some(v) = idx.open_visit.clone() {
                    let keyed = v.pars_ms.is_some() || looks_keyed(&v.name);
                    let aborted = keyed && v.success.is_none();
                    self.enqueue_metas(
                        &LogRef { path: path.clone() },
                        Vec::new(),
                        Vec::new(),
                        aborted.then(|| v.clone()),
                    );
                    if !aborted {
                        overalls.push(v);
                    }
                }
                (idx.open.clone(), overalls)
            };
            self.enqueue_metas(&LogRef { path }, idx.segments, overalls, open);
        }
        self.dispatch();
    }
}

/// `"Skyreach +10"` — the display name a keyed visit's Overall wears.
fn looks_keyed(name: &str) -> bool {
    name.rsplit_once(" +")
        .is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// What the store needs to know about a log once: its identity and its
/// timezone offset, both from the first complete line.
#[derive(Debug, Clone, Copy)]
pub struct LogFacts {
    pub id: u64,
    pub tz_min: Option<i16>,
    /// The first line was complete, so `id` is the log's real identity.
    /// A half-written header (the daemon retargets the instant a file
    /// appears, and the game flushes in bursts) yields a filename-hash id
    /// that must not be remembered: the next look may see the header.
    pub complete: bool,
}

impl LogFacts {
    pub fn read(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let first = std::fs::File::open(path).ok().and_then(|f| {
            let mut line = String::new();
            BufReader::new(f).read_line(&mut line).ok()?;
            // A complete line ends in a newline; half a line is not yet an
            // identity (the daemon retargets the instant a file appears).
            line.ends_with('\n').then_some(line)
        });
        Self {
            id: log_id(first.as_deref(), &name),
            tz_min: first.as_deref().and_then(tz_offset_min),
            complete: first.is_some(),
        }
    }
}

/// Reassemble a `ClosedFight` from an import job's parsed slice — the same
/// meter the engine would show for that segment.
fn fight_from_import(job: &ImportJob, meter: &Meter) -> Option<ClosedFight> {
    let (segment, visit) = match job.meta.kind {
        SegmentKind::Overall => {
            let ord = job.meta.visit?;
            let seg = meter.overall(ord)?;
            let visit = meter.visits().get(ord as usize).cloned();
            (seg, visit)
        }
        SegmentKind::Encounter | SegmentKind::Trash => {
            // The slice reproduces exactly its segment; pick it by start.
            let seg = meter
                .segments()
                .iter()
                .find(|s| s.start_ms == job.meta.start_ms)
                .or_else(|| meter.segments().first())?
                .clone();
            (seg, None)
        }
    };
    Some(ClosedFight {
        segment,
        visit,
        log: job.log.clone(),
        byte_range: Some(job.meta.byte_range),
        aborted: job.aborted,
    })
}

// ---- backends -------------------------------------------------------------------

/// Where the documents live. Directory in production, memory for the mock
/// and the tests. `dir` is one of `fights`, `rows`, `details`, `loadouts`,
/// `annotations`; `name` is the file name.
pub trait Backend {
    fn list(&self, dir: &str) -> Vec<String>;
    fn read(&self, dir: &str, name: &str) -> Option<Vec<u8>>;
    fn exists(&self, dir: &str, name: &str) -> bool;
    fn write(&mut self, dir: &str, name: &str, bytes: &[u8]) -> io::Result<()>;
    fn remove(&mut self, dir: &str, name: &str) -> io::Result<()>;
}

pub struct DirBackend {
    root: PathBuf,
}

impl DirBackend {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Backend for DirBackend {
    fn list(&self, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root.join(dir))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with(".tmp"))
            .collect();
        names.sort();
        names
    }

    fn read(&self, dir: &str, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.root.join(dir).join(name)).ok()
    }

    fn exists(&self, dir: &str, name: &str) -> bool {
        self.root.join(dir).join(name).exists()
    }

    fn write(&mut self, dir: &str, name: &str, bytes: &[u8]) -> io::Result<()> {
        let d = self.root.join(dir);
        std::fs::create_dir_all(&d)?;
        write_atomic(&d.join(name), bytes)
    }

    fn remove(&mut self, dir: &str, name: &str) -> io::Result<()> {
        match std::fs::remove_file(self.root.join(dir).join(name)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}

#[derive(Default)]
pub struct MemBackend {
    files: BTreeMap<(String, String), Vec<u8>>,
    /// Simulate ENOSPC / an unwritable directory.
    pub fail_writes: bool,
}

impl MemBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Backend for MemBackend {
    fn list(&self, dir: &str) -> Vec<String> {
        self.files
            .keys()
            .filter(|(d, _)| d == dir)
            .map(|(_, n)| n.clone())
            .collect()
    }

    fn read(&self, dir: &str, name: &str) -> Option<Vec<u8>> {
        self.files
            .get(&(dir.to_string(), name.to_string()))
            .cloned()
    }

    fn exists(&self, dir: &str, name: &str) -> bool {
        self.files
            .contains_key(&(dir.to_string(), name.to_string()))
    }

    fn write(&mut self, dir: &str, name: &str, bytes: &[u8]) -> io::Result<()> {
        if self.fail_writes {
            return Err(io::Error::other("simulated write failure"));
        }
        self.files
            .insert((dir.to_string(), name.to_string()), bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, dir: &str, name: &str) -> io::Result<()> {
        self.files.remove(&(dir.to_string(), name.to_string()));
        Ok(())
    }
}

// ---- the store --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retention {
    pub store_trash: bool,
    pub keep_per_encounter: usize,
    pub keep_details_per_encounter: usize,
    pub characters: Vec<String>,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            store_trash: false,
            keep_per_encounter: 200,
            keep_details_per_encounter: 10,
            characters: Vec::new(),
        }
    }
}

impl From<&HistoryOptions> for Retention {
    fn from(o: &HistoryOptions) -> Self {
        Self {
            store_trash: o.store_trash,
            keep_per_encounter: o.keep_per_encounter,
            keep_details_per_encounter: o.keep_details_per_encounter,
            characters: o.characters.clone(),
        }
    }
}

/// The files plus their in-memory index. Generic over the backend so
/// `daemon::mock` drives it synchronously in memory.
pub struct Store<B: Backend> {
    backend: B,
    cfg: Retention,
    cards: Vec<FightCard>,
    /// Reported once in `Status`: the latest write/read failure.
    pub last_error: Option<String>,
    corrupt: u32,
}

impl<B: Backend> Store<B> {
    /// Rebuild the index from `fights/`. Unreadable cards are skipped and
    /// counted; the rest are served.
    pub fn open(backend: B, cfg: Retention) -> Self {
        let mut cards = Vec::new();
        let mut corrupt = 0u32;
        for name in backend.list("fights") {
            let parsed = backend
                .read("fights", &name)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|text| json::parse(&text).ok())
                .and_then(|v| FightCard::from_json(&v));
            match parsed {
                Some(card) => cards.push(card),
                None => corrupt += 1,
            }
        }
        cards.sort_by_key(|c| c.start_utc_ms);
        let last_error =
            (corrupt > 0).then(|| format!("{corrupt} unreadable card(s) in fights/ skipped"));
        Self {
            backend,
            cfg,
            cards,
            last_error,
            corrupt,
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Every card, oldest first.
    pub fn cards(&self) -> &[FightCard] {
        &self.cards
    }

    pub fn has(&self, id: &str) -> bool {
        self.cards.iter().any(|c| c.id == id)
    }

    pub fn card(&self, id: &str) -> Option<&FightCard> {
        self.cards.iter().find(|c| c.id == id)
    }

    pub fn rows(&self, id: &str) -> Option<FightRows> {
        self.read_doc("rows", id)
            .and_then(|v| FightRows::from_json(&v))
    }

    pub fn details(&self, id: &str) -> Option<FightDetails> {
        self.read_doc("details", id)
            .and_then(|v| FightDetails::from_json(&v))
    }

    pub fn has_details(&self, id: &str) -> bool {
        self.backend.exists("details", &format!("{id}.json"))
    }

    pub fn loadout(&self, hash: u64) -> Option<StoredLoadout> {
        self.read_doc("loadouts", &format!("{hash:016x}"))
            .and_then(|v| StoredLoadout::from_json(&v))
    }

    fn read_doc(&self, dir: &str, stem: &str) -> Option<json::Json> {
        let bytes = self.backend.read(dir, &format!("{stem}.json"))?;
        json::parse(&String::from_utf8(bytes).ok()?).ok()
    }

    pub fn status(&self) -> HistoryStatus {
        HistoryStatus {
            enabled: true,
            fights: self.cards.len() as u32,
            dropped: 0,
            importing: 0,
            owner_inferred: self.cfg.characters.is_empty() && self.owner().is_some(),
            error: self.last_error.clone(),
        }
    }

    /// Would this scanned segment be stored at all? A pre-filter for the
    /// import path (`keyed` = visit ordinals that were keystone runs, so a
    /// key's bosses are not parsed only to be refused); `wants` is the truth.
    pub fn wants_meta(&self, meta: &SegmentMeta, keyed: &HashSet<u32>) -> bool {
        match meta.kind {
            SegmentKind::Overall => true,
            SegmentKind::Encounter => {
                self.cfg.store_trash || !meta.visit.is_some_and(|v| keyed.contains(&v))
            }
            SegmentKind::Trash => self.cfg.store_trash && meta.counts,
        }
    }

    /// Encounters and Overalls always; a keyed run's member bosses and
    /// Trash only under the trash switch (spec §6); noise never.
    fn wants(&self, seg: &Segment, visit: Option<&Visit>) -> bool {
        if seg.noise {
            return false;
        }
        match seg.kind {
            SegmentKind::Overall => true,
            SegmentKind::Encounter => self.cfg.store_trash || !visit.is_some_and(|v| v.keyed),
            SegmentKind::Trash => self.cfg.store_trash && seg.counts(),
        }
    }

    /// Insert-if-absent on the fight id (a record is rewritten only when
    /// its schema is older). Returns the id when something was written.
    pub fn store(&mut self, fight: &ClosedFight, facts: LogFacts) -> Option<String> {
        if !self.wants(&fight.segment, fight.visit.as_ref()) {
            return None;
        }
        let id = fight_id(facts.id, fight.segment.start_ms);
        // An aborted record is provisional: the same fight closing for real
        // (its END arriving after a restart) replaces it.
        if let Some(existing) = self.card(&id)
            && existing.schema >= HISTORY_SCHEMA
            && !(existing.aborted && !fight.aborted)
        {
            return None;
        }
        let owner = self.owner();
        let mut doc = extract(fight, facts, &id);
        doc.card.owner = owner.map(|(guid, _)| guid);
        // The rows tier always; details for kills (retention keeps bests
        // and pins afterwards); loadouts content-addressed.
        let write = |b: &mut B, dir: &str, stem: &str, v: json::Json| -> io::Result<()> {
            b.write(dir, &format!("{stem}.json"), v.to_line().as_bytes())
        };
        let mut result = write(&mut self.backend, "rows", &id, doc.rows.to_json());
        if result.is_ok() && doc.card.success == Some(true) {
            result = write(&mut self.backend, "details", &id, doc.details.to_json());
        }
        for l in &doc.loadouts {
            let name = format!("{:016x}.json", l.hash);
            if result.is_ok() && !self.backend.exists("loadouts", &name) {
                result = self
                    .backend
                    .write("loadouts", &name, l.to_json().to_line().as_bytes());
            }
        }
        // The card last: its presence is what makes the fight exist.
        if result.is_ok() {
            result = write(&mut self.backend, "fights", &id, doc.card.to_json());
        }
        if let Err(e) = result {
            self.last_error = Some(format!("history write failed: {e}"));
            return None;
        }
        self.cards.retain(|c| c.id != id);
        let at = self
            .cards
            .partition_point(|c| c.start_utc_ms <= doc.card.start_utc_ms);
        self.cards.insert(at, doc.card);
        self.retain();
        Some(id)
    }

    /// Flip a card's pin — the one in-place card edit.
    pub fn pin(&mut self, id: &str, pinned: bool) -> bool {
        let Some(card) = self.cards.iter_mut().find(|c| c.id == id) else {
            return false;
        };
        card.pinned = pinned;
        let doc = card.to_json().to_line();
        match self
            .backend
            .write("fights", &format!("{id}.json"), doc.as_bytes())
        {
            Ok(()) => true,
            Err(e) => {
                self.last_error = Some(format!("history write failed: {e}"));
                false
            }
        }
    }

    /// Who "me" is: the configured character, else the one guid every
    /// stored log's COMBATANT_INFO named (spec §9). `(guid, inferred)`.
    pub fn owner(&self) -> Option<(String, bool)> {
        if !self.cfg.characters.is_empty() {
            let wanted: Vec<String> = self
                .cfg
                .characters
                .iter()
                .map(|c| c.trim().to_lowercase())
                .filter(|c| !c.is_empty())
                .collect();
            return self
                .cards
                .iter()
                .rev()
                .flat_map(|c| c.players.iter())
                .find(|p| {
                    // "Name-Realm" must match whole; a bare "Name" (no
                    // realm given) matches the name half.
                    let full = p.name.to_lowercase();
                    let bare = full.split('-').next().unwrap_or(&full);
                    wanted
                        .iter()
                        .any(|w| w == &full || (!w.contains('-') && w == bare))
                })
                .map(|p| (p.guid.clone(), false));
        }
        let mut per_log: HashMap<u64, HashSet<&str>> = HashMap::new();
        for c in &self.cards {
            let set = per_log.entry(c.log).or_default();
            for p in c.players.iter().filter(|p| p.logged && !p.enemy) {
                set.insert(&p.guid);
            }
        }
        let mut logs = per_log.values().filter(|s| !s.is_empty());
        let mut common: HashSet<&str> = logs.next()?.clone();
        for s in logs {
            common.retain(|g| s.contains(g));
        }
        // One log alone can't tell the logger from their guildmates; two
        // can only when exactly one name survives the intersection.
        if per_log.len() >= 2 && common.len() == 1 {
            common.into_iter().next().map(|g| (g.to_string(), true))
        } else {
            None
        }
    }

    /// Cards + rows per (kind, encounter or map, difficulty) capped at
    /// `keep_per_encounter`, details at `keep_details_per_encounter`,
    /// oldest first, never touching the protected set: pinned, annotated,
    /// the fastest kill per group, and the owner's best per_sec per
    /// (group, spec) for Damage and Healing.
    fn retain(&mut self) {
        let protected = self.protected();
        let mut groups: BTreeMap<(u8, u32, u32), Vec<usize>> = BTreeMap::new();
        for (i, c) in self.cards.iter().enumerate() {
            groups.entry(group_key(c)).or_default().push(i);
        }
        let mut evict: Vec<usize> = Vec::new();
        let mut demote: Vec<String> = Vec::new();
        for idxs in groups.values() {
            // Oldest first already (cards are sorted by start).
            let unprotected: Vec<usize> = idxs
                .iter()
                .copied()
                .filter(|i| {
                    !self
                        .cards
                        .get(*i)
                        .is_some_and(|c| protected.contains(&c.id))
                })
                .collect();
            let over = idxs.len().saturating_sub(self.cfg.keep_per_encounter);
            evict.extend(unprotected.iter().take(over));
            let with_details: Vec<usize> = idxs
                .iter()
                .copied()
                .filter(|i| self.cards.get(*i).is_some_and(|c| self.has_details(&c.id)))
                .collect();
            let over = with_details
                .len()
                .saturating_sub(self.cfg.keep_details_per_encounter);
            demote.extend(
                with_details
                    .iter()
                    .filter(|i| unprotected.contains(i))
                    .take(over)
                    .filter_map(|i| self.cards.get(*i).map(|c| c.id.clone())),
            );
        }
        for id in demote {
            let _ = self.backend.remove("details", &format!("{id}.json"));
        }
        evict.sort_unstable();
        for i in evict.into_iter().rev() {
            if i < self.cards.len() {
                let card = self.cards.remove(i);
                for dir in ["details", "rows", "fights"] {
                    let _ = self.backend.remove(dir, &format!("{}.json", card.id));
                }
            }
        }
    }

    fn protected(&self) -> HashSet<String> {
        let mut out: HashSet<String> = HashSet::new();
        let owner = self.owner().map(|(g, _)| g);
        let mut fastest: HashMap<GroupKey, (i64, &str)> = HashMap::new();
        // (group, spec id, 0 = damage / 1 = healing) → the owner's best per_sec.
        let mut best: HashMap<(GroupKey, u32, u8), (f64, &str)> = HashMap::new();
        for c in &self.cards {
            if c.pinned
                || self
                    .backend
                    .exists("annotations", &format!("{}.ndjson", c.id))
            {
                out.insert(c.id.clone());
            }
            let key = group_key(c);
            if c.success == Some(true) && !c.aborted {
                let e = fastest.entry(key).or_insert((c.duration_ms, &c.id));
                if c.duration_ms < e.0 {
                    *e = (c.duration_ms, &c.id);
                }
            }
            if let Some(owner) = &owner
                && let Some(p) = c.players.iter().find(|p| &p.guid == owner)
            {
                let spec = p.spec.map_or(0, |s| s.id());
                for (view, per_sec) in [(0u8, p.dps), (1u8, p.hps)] {
                    let e = best.entry((key, spec, view)).or_insert((per_sec, &c.id));
                    if per_sec > e.0 {
                        *e = (per_sec, &c.id);
                    }
                }
            }
        }
        out.extend(fastest.values().map(|(_, id)| id.to_string()));
        out.extend(best.values().map(|(_, id)| id.to_string()));
        out
    }

    /// Cards the import path should not re-parse: everything, by id.
    pub fn ids(&self) -> HashSet<String> {
        self.cards.iter().map(|c| c.id.clone()).collect()
    }

    // ---- the fixed questions (spec §8) ---------------------------------------

    pub fn answer(&self, q: &HistoryQuery) -> HistoryAnswer {
        match q {
            HistoryQuery::Fights {
                encounter,
                difficulty,
                guid,
                since_utc_ms,
                kind,
                sort,
                limit,
            } => HistoryAnswer::Fights(self.fights(
                *encounter,
                *difficulty,
                guid.as_deref(),
                *since_utc_ms,
                *kind,
                *sort,
                *limit,
            )),
            HistoryQuery::Progression {
                encounter,
                difficulty,
            } => self.progression(*encounter, *difficulty),
            HistoryQuery::Trend {
                guid,
                spec,
                encounter,
                difficulty,
                view,
                bucket,
                since_utc_ms,
                limit,
            } => HistoryAnswer::Trend(self.trend(
                guid,
                *spec,
                *encounter,
                *difficulty,
                *view,
                *bucket,
                *since_utc_ms,
                *limit,
            )),
        }
    }

    /// `Fastest` considers kills only (best kill = `Fastest`, limit 1);
    /// `OwnerPerSec` ranks by the owner's damage per second and needs an
    /// owner; `limit` 0 means 50.
    #[allow(clippy::too_many_arguments)]
    fn fights(
        &self,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        guid: Option<&str>,
        since_utc_ms: Option<i64>,
        kind: Option<FightKind>,
        sort: FightSort,
        limit: u32,
    ) -> Vec<FightCard> {
        let owner = self.owner().map(|(g, _)| g);
        let mut hits: Vec<&FightCard> = self
            .cards
            .iter()
            .filter(|c| encounter.is_none_or(|e| c.encounter.is_some_and(|x| x.id == e)))
            .filter(|c| difficulty.is_none_or(|d| card_difficulty(c) == Some(d)))
            .filter(|c| guid.is_none_or(|g| c.players.iter().any(|p| p.guid == g)))
            .filter(|c| since_utc_ms.is_none_or(|s| c.start_utc_ms >= s))
            .filter(|c| kind.is_none_or(|k| c.kind == k))
            .filter(|c| sort != FightSort::Fastest || (c.success == Some(true) && !c.aborted))
            .collect();
        match sort {
            FightSort::Newest => hits.sort_by_key(|c| std::cmp::Reverse(c.start_utc_ms)),
            FightSort::Fastest => hits.sort_by_key(|c| (c.duration_ms, c.start_utc_ms)),
            FightSort::OwnerPerSec => {
                let per_sec = |c: &FightCard| {
                    owner
                        .as_deref()
                        .and_then(|o| c.players.iter().find(|p| p.guid == o))
                        .map_or(0.0, |p| p.dps)
                };
                hits.sort_by(|a, b| {
                    per_sec(b)
                        .partial_cmp(&per_sec(a))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(b.start_utc_ms.cmp(&a.start_utc_ms))
                });
            }
        }
        let limit = if limit == 0 { 50 } else { limit as usize };
        hits.into_iter().take(limit).cloned().collect()
    }

    fn progression(&self, encounter: u32, difficulty: u32) -> HistoryAnswer {
        let pulls: Vec<&FightCard> = self
            .cards
            .iter()
            .filter(|c| {
                c.encounter
                    .is_some_and(|e| e.id == encounter && e.difficulty == difficulty)
                    && !c.aborted
            })
            .collect();
        let kills: Vec<&FightCard> = pulls
            .iter()
            .copied()
            .filter(|c| c.success == Some(true))
            .collect();
        let first_kill = kills
            .iter()
            .min_by_key(|c| c.start_utc_ms)
            .map(|c| Box::new((*c).clone()));
        let mut nights: BTreeMap<i64, Night> = BTreeMap::new();
        for c in &pulls {
            let day = c.start_utc_ms.div_euclid(DAY_MS) * DAY_MS;
            let n = nights.entry(day).or_insert(Night {
                day_utc_ms: day,
                pulls: 0,
                kill: false,
                best_pct: None,
            });
            n.pulls += 1;
            n.kill |= c.success == Some(true);
            // R16: the night's lowest.
            n.best_pct = match (n.best_pct, c.best_pct) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        let mut durations: Vec<i64> = kills.iter().map(|c| c.duration_ms).collect();
        durations.sort_unstable();
        let median_kill_ms = match durations.len() {
            0 => None,
            n if n % 2 == 1 => durations.get(n / 2).copied(),
            n => durations
                .get(n / 2 - 1)
                .zip(durations.get(n / 2))
                .map(|(a, b)| (a + b) / 2),
        };
        HistoryAnswer::Progression {
            pulls: pulls.len() as u32,
            kills: kills.len() as u32,
            first_kill,
            nights: nights.into_values().collect(),
            median_kill_ms,
        }
    }

    /// One point per fight (newest first), or per UTC day / week with
    /// `per_sec` averaged and `amount` / `duration_ms` summed.
    #[allow(clippy::too_many_arguments)]
    fn trend(
        &self,
        guid: &str,
        spec: Option<u32>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        view: View,
        bucket: TrendBucket,
        since_utc_ms: Option<i64>,
        limit: u32,
    ) -> Vec<TrendPoint> {
        let mut points: Vec<TrendPoint> = self
            .cards
            .iter()
            .filter(|c| !c.aborted)
            .filter(|c| encounter.is_none_or(|e| c.encounter.is_some_and(|x| x.id == e)))
            .filter(|c| difficulty.is_none_or(|d| card_difficulty(c) == Some(d)))
            .filter(|c| since_utc_ms.is_none_or(|s| c.start_utc_ms >= s))
            .filter_map(|c| {
                let p = c.players.iter().find(|p| p.guid == guid)?;
                let p_spec = p.spec.map(|s| s.id());
                if spec.is_some() && p_spec != spec {
                    return None;
                }
                let (amount, per_sec) = match view {
                    View::Healing => (p.healing, p.hps),
                    _ => (p.damage, p.dps),
                };
                Some(TrendPoint {
                    bucket_utc_ms: match bucket {
                        TrendBucket::None => c.start_utc_ms,
                        TrendBucket::Day => c.start_utc_ms.div_euclid(DAY_MS) * DAY_MS,
                        // Epoch day 0 was a Thursday; shift so weeks start Monday.
                        TrendBucket::Week => {
                            (c.start_utc_ms - 4 * DAY_MS).div_euclid(7 * DAY_MS) * 7 * DAY_MS
                                + 4 * DAY_MS
                        }
                    },
                    fight_id: c.id.clone(),
                    spec: p_spec,
                    amount,
                    per_sec,
                    duration_ms: c.duration_ms,
                    n: 1,
                })
            })
            .collect();
        if bucket != TrendBucket::None {
            let mut folded: BTreeMap<i64, TrendPoint> = BTreeMap::new();
            for p in points {
                match folded.get_mut(&p.bucket_utc_ms) {
                    Some(f) => {
                        f.per_sec = (f.per_sec * f64::from(f.n) + p.per_sec) / f64::from(f.n + 1);
                        f.amount += p.amount;
                        f.duration_ms += p.duration_ms;
                        f.n += 1;
                        // The newest fight names the bucket.
                        f.fight_id = p.fight_id;
                        if p.spec.is_some() {
                            f.spec = p.spec;
                        }
                    }
                    None => {
                        folded.insert(p.bucket_utc_ms, p);
                    }
                }
            }
            points = folded.into_values().collect();
        }
        points.sort_by_key(|p| std::cmp::Reverse(p.bucket_utc_ms));
        let limit = if limit == 0 { 50 } else { limit as usize };
        points.truncate(limit);
        points
    }

    /// The card plus the view's rows, and the drilled player's breakdown:
    /// by-spell / by-target and their timeline from the details tier for
    /// Damage and Healing (absent when demoted), the death recap from the
    /// rows tier for Deaths.
    pub fn stored_fight(&self, id: &str, view: View, drill: Option<&str>) -> Option<StoredFight> {
        let card = self.card(id)?.clone();
        let rows_doc = self.rows(id)?;
        let rows = rows_doc.rows(view).to_vec();
        let breakdown = drill.and_then(|guid| match view {
            View::Deaths => rows_doc
                .recaps
                .iter()
                .find(|r| r.guid == guid)
                .map(|r| Breakdown {
                    by_spell: r.events.clone(),
                    by_target: r.attackers.clone(),
                    ..Breakdown::default()
                }),
            View::Damage | View::Healing => {
                let details = self.details(id)?;
                let p = details.players.into_iter().find(|p| p.guid == guid)?;
                Some(if view == View::Damage {
                    Breakdown {
                        by_spell: p.damage_spells,
                        by_target: p.damage_targets,
                        timeline: Some(p.damage_timeline),
                        ..Breakdown::default()
                    }
                } else {
                    Breakdown {
                        by_spell: p.heal_spells,
                        by_target: p.heal_targets,
                        timeline: Some(p.heal_timeline),
                        ..Breakdown::default()
                    }
                })
            }
            _ => None,
        });
        Some(StoredFight {
            card,
            rows,
            breakdown,
        })
    }

    pub fn corrupt(&self) -> u32 {
        self.corrupt
    }
}

const DAY_MS: i64 = 86_400_000;

/// The difficulty a query filters on: the encounter's, else the visit's.
fn card_difficulty(c: &FightCard) -> Option<u32> {
    c.encounter
        .map(|e| e.difficulty)
        .or_else(|| c.key.as_ref().map(|k| k.difficulty))
}

/// Retention group: `(kind, encounter id | map id, difficulty)`.
type GroupKey = (u8, u32, u32);

fn group_key(c: &FightCard) -> GroupKey {
    let kind = match c.kind {
        FightKind::Encounter => 0,
        FightKind::Arena => 1,
        FightKind::Key => 2,
        FightKind::Overall => 3,
        FightKind::Trash => 4,
    };
    match (c.encounter, &c.key) {
        (Some(e), _) => (kind, e.id, e.difficulty),
        (None, Some(k)) => (kind, k.map_id, k.difficulty.max(k.level.unwrap_or(0))),
        (None, None) => (kind, 0, 0),
    }
}

// ---- extraction ---------------------------------------------------------------------

/// Everything one stored fight consists of.
pub struct FightDocs {
    pub card: FightCard,
    pub rows: FightRows,
    pub details: FightDetails,
    pub loadouts: Vec<StoredLoadout>,
}

/// Derive every document from the segment — the same calls a snapshot
/// makes, nothing an event store would need.
pub fn extract(fight: &ClosedFight, facts: LogFacts, id: &str) -> FightDocs {
    let seg = &fight.segment;
    let now = seg.last_combat_ms();
    let kind = match seg.kind {
        SegmentKind::Encounter if seg.arena => FightKind::Arena,
        SegmentKind::Encounter => FightKind::Encounter,
        SegmentKind::Overall if fight.visit.as_ref().is_some_and(|v| v.keyed) => FightKind::Key,
        SegmentKind::Overall => FightKind::Overall,
        SegmentKind::Trash => FightKind::Trash,
    };
    let aborted = fight.aborted
        || (matches!(kind, FightKind::Encounter | FightKind::Arena) && seg.success.is_none());

    let mut views: [Vec<wowdps_core::model::Row>; View::COUNT] = Default::default();
    for (slot, (view, _)) in views
        .iter_mut()
        .zip(wowdps_proto::history::VIEW_KEYS.iter())
    {
        *slot = seg.rows(*view);
    }
    let by_view = |v: View| views.get(v.index()).map_or(&[][..], Vec::as_slice);

    // Players: the union of everyone with a meter row, denormalized.
    let mut order: Vec<String> = Vec::new();
    let mut players: HashMap<String, CardPlayer> = HashMap::new();
    for view in [View::Damage, View::Healing, View::Deaths] {
        for r in by_view(view) {
            let p = players.entry(r.key.clone()).or_insert_with(|| {
                order.push(r.key.clone());
                CardPlayer {
                    guid: r.key.clone(),
                    name: r.label.clone(),
                    class: r.class,
                    spec: r.spec,
                    enemy: r.enemy,
                    ..CardPlayer::default()
                }
            });
            match view {
                View::Damage => {
                    p.damage = r.amount;
                    p.dps = r.per_sec;
                }
                View::Healing => {
                    p.healing = r.amount;
                    p.hps = r.per_sec;
                }
                _ => p.deaths = u32::try_from(r.amount).unwrap_or(u32::MAX),
            }
            if p.class.is_none() {
                p.class = r.class;
            }
            if p.spec.is_none() {
                p.spec = r.spec;
            }
        }
    }
    let mut loadouts: Vec<StoredLoadout> = Vec::new();
    for guid in &order {
        if let Some(p) = players.get_mut(guid)
            && let Some(l) = seg.loadout(guid)
        {
            let hash = loadout_hash(l);
            p.loadout = Some(hash);
            p.logged = true;
            if !loadouts.iter().any(|s| s.hash == hash) {
                loadouts.push(StoredLoadout::new(l.clone()));
            }
        }
    }
    let players: Vec<CardPlayer> = order.iter().filter_map(|g| players.remove(g)).collect();

    let recaps: Vec<Recap> = players
        .iter()
        .filter(|p| p.deaths > 0)
        .map(|p| {
            let (events, attackers) = seg.breakdown(&p.guid, View::Deaths);
            Recap {
                guid: p.guid.clone(),
                events,
                attackers,
            }
        })
        .collect();
    let details: Vec<PlayerDetail> = players
        .iter()
        .filter(|p| !p.enemy)
        .map(|p| {
            let (damage_spells, damage_targets) = seg.breakdown(&p.guid, View::Damage);
            let (heal_spells, heal_targets) = seg.breakdown(&p.guid, View::Healing);
            PlayerDetail {
                guid: p.guid.clone(),
                damage_spells,
                damage_targets,
                heal_spells,
                heal_targets,
                damage_timeline: seg.timeline(&p.guid),
                heal_timeline: seg.heal_timeline(&p.guid),
            }
        })
        .collect();

    let tz = facts.tz_min;
    let start_utc_ms = seg.start_ms - i64::from(tz.unwrap_or(0)) * 60_000;
    let friendly = players.iter().filter(|p| !p.enemy).map(|p| p.guid.as_str());
    let card = FightCard {
        schema: HISTORY_SCHEMA,
        id: id.to_string(),
        log: facts.id,
        content: content_id(seg.encounter, start_utc_ms, friendly),
        kind,
        name: seg.name.clone(),
        encounter: seg.encounter,
        key: fight.visit.as_ref().map(|v| KeyInfo {
            map_id: v.map_id,
            difficulty: v.difficulty,
            level: v.key_level,
            completed: v.completed,
        }),
        start_local_ms: seg.start_ms,
        tz_min: tz,
        start_utc_ms,
        duration_ms: seg.duration_ms(now),
        official_ms: fight.visit.as_ref().and_then(|v| v.official_ms),
        pars_ms: fight.visit.as_ref().and_then(|v| v.pars_ms),
        success: if aborted { None } else { seg.success },
        aborted,
        build: seg.build,
        project_id: seg.project_id,
        log_version: seg.log_version,
        owner: None,
        byte_range: fight.byte_range,
        pinned: false,
        best_pct: seg.best_pct(),
        players,
    };
    FightDocs {
        card,
        rows: FightRows {
            schema: HISTORY_SCHEMA,
            id: id.to_string(),
            views,
            recaps,
        },
        details: FightDetails {
            schema: HISTORY_SCHEMA,
            id: id.to_string(),
            players: details,
        },
        loadouts,
    }
}
