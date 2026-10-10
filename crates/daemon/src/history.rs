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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use wowdps_core::index::{self, SegmentMeta};
use wowdps_core::meter::{Meter, Segment, SegmentKind, Visit};
use wowdps_core::model::replay::Cut;
use wowdps_core::model::series::{self as series_model, SpellTallies, snap, window_rows};
use wowdps_core::model::{
    Class, Role, RoleNightRow, Row, SegmentId, ShieldRow, Spec, Timeline, View,
};
use wowdps_core::parser::tz_offset_min;
use wowdps_core::tail::{SourceSpec, newest_log};
use wowdps_proto::creatures::{self, Creature};
use wowdps_proto::history::{
    Affiliation, COARSE_BUCKET_MS, COUNT_VIEWS, CardPlayer, CountDetail, FightCard, FightDetails,
    FightKind, FightRows, HISTORY_SCHEMA, KeyBoss, KeyInfo, PlayerCoarse, PlayerDetail,
    PlayerMitigation, PlayerShields, PlayerStacks, PlayerSupport, PlayerUptime, Recap,
    StoredLoadout, TAKEN_SPELLS_CAP, TakenOther, addon_table, content_id, fight_id, loadout_hash,
    log_id, sigma_id,
};
use wowdps_proto::json;
use wowdps_proto::msg::{DeathWindow, HistoryStatus};
use wowdps_proto::replay as replay_tier;
use wowdps_proto::series::{self as series_tier, FightSeries, PlayerSeries};
use wowdps_proto::{
    Breakdown, CompareSide, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery, Night, StoredFight,
    StoredPair, StoredUptime, TrendBucket, TrendMeasure, TrendPoint,
};

use crate::addon;
use crate::cache::{IndexCache, write_atomic};
use crate::hub::HubMsg;
use crate::loader::{LoadReply, LoadReq};
use crate::mine::Mine;

/// Bound of the hub → history channel. A night's worth of pulls is a few
/// dozen; 64 in flight means the thread is wedged, and dropping (counted)
/// beats stalling the meter.
pub const QUEUE: usize = 64;

/// How many of [`QUEUE`]'s slots a client's *reads* may ever occupy at once.
/// Reads (`Query`/`Fight`) come from clients and can arrive in a loop — a
/// dashboard paging the store, say — while a `Store` arrives exactly once per
/// closed fight and is the only request whose loss costs the user data.
/// Reserving the rest of the queue for writes means a read flood can never
/// take the slot a closing pull needs: the read is dropped (and counted)
/// instead, and the client still gets its empty answer from the hub.
const READ_QUOTA: usize = QUEUE / 2;

/// Most cards one `Fights` answer may carry, whatever `limit` asked for.
/// See `Store::fights` for why the ceiling is the daemon's job.
pub const FIGHTS_CAP: usize = 500;

/// Difficulty.db2 id of a Mythic Keystone pull: a boss at this difficulty is
/// a key's member even when the key's START predates the log (the daemon
/// attached mid-run), so it is stored only under the trash switch.
const KEYSTONE_DIFFICULTY: u32 = 8;

/// `history_*` keys of `~/.config/wowdps/config.toml`, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryOptions {
    pub dir: PathBuf,
    pub store_trash: bool,
    pub keep_per_encounter: usize,
    pub keep_details_per_encounter: usize,
    /// A wipe at least this long (seconds) gets a details file too (spec §6).
    pub details_min_wipe_secs: u64,
    /// v42: every boss kill and timed key kept whole (`Retention::keeps_whole`).
    pub keep_kills_whole: bool,
    /// v45: every wipe on a boss the store has not seen killed at that
    /// difficulty kept whole too (`Store::is_progression`).
    pub keep_progression_whole: bool,
    /// v45 (R29): the replay tier's size cap in MiB (0 = none).
    pub replay_mb: u64,
    /// "Name-Realm" strings that are "me" (spec §9); empty = infer.
    pub characters: Vec<String>,
    /// The index-checkpoint cache, so the start-up sweep of old logs costs
    /// a tail rescan, not a full one.
    pub cache_dir: Option<PathBuf>,
    /// The game's product directory (`<install>/_retail_`) the tailed logs
    /// belong to — where the wowdps addon lives and where its
    /// SavedVariables land (spec §9a). `None` when the source is not
    /// inside an install: no addon check, no affiliations.
    pub addon_dir: Option<PathBuf>,
}

/// How often the thread stats the addon's SavedVariables for a new write
/// while nothing else is happening. The game writes them on logout, so a
/// minute's lag is nothing; a stat is nothing either.
const SAVED_VARIABLES_POLL: Duration = Duration::from_secs(30);

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
    /// A keyed visit's member bosses (Encounter segments), pull order —
    /// listed on the key's card so a reader can drill into them.
    pub members: Vec<KeyBoss>,
}

/// One historical segment the import path asked the loader pool to parse.
#[derive(Debug, Clone)]
pub struct ImportJob {
    pub log: LogRef,
    pub meta: SegmentMeta,
    pub aborted: bool,
    /// Rewrite the card even though the store has it (a regrade).
    pub regrade: bool,
    /// A boss drill: answer this session with the parsed member's own
    /// rows / breakdown instead of storing anything.
    pub drill: Option<DrillReq>,
    /// v45 (R29): whether the replay is cut from the same lines.
    pub cut: CutJob,
}

/// v45 (R29): what an import job makes beside (or instead of) the meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutJob {
    /// The meter alone: a boss drill, trash, an arena, a raid night's Σ.
    No,
    /// The meter and the replay cut, from one parse of the lines: a boss
    /// pull or a keystone run being stored or rewritten.
    Also,
    /// The replay cut alone: the card and its tiers are current, its replay
    /// is not (a live pull just closed, a fight older than the tier).
    Only,
}

impl CutJob {
    /// What a scanned segment's import cuts: a boss pull (not an arena's)
    /// and a keystone run's Σ are cut beside their meter.
    pub fn of(meta: &SegmentMeta, keyed: bool) -> Self {
        let cut = match meta.kind {
            SegmentKind::Encounter => !meta.arena,
            SegmentKind::Overall => keyed,
            SegmentKind::Trash => false,
        };
        if cut { CutJob::Also } else { CutJob::No }
    }
}

/// v45: what the loader pool made of an import job: the segment's meter
/// (none on a [`CutJob::Only`] job) and its replay cut (none unless asked).
pub struct Loaded {
    pub meter: Option<Box<Meter>>,
    pub cut: Option<Box<Cut>>,
}

/// Who asked for a member boss, and what of it.
#[derive(Debug, Clone)]
pub struct DrillReq {
    pub session: u64,
    pub req_id: u32,
    pub ask: Ask,
}

/// v42: what a `GetFight` asks of a fight — the view's rows; with a drill,
/// that player's lists and their levels (a death window, an opened
/// ability), a zoom window, a second player to compare them with — and
/// whether the asker stacks a drill's graph (`engine::wants_series`: the
/// window does; the mcp, the overlay and the TUI read no stack).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub view: View,
    pub drill: Option<String>,
    /// v28 (R9): which death window a Deaths drill describes.
    pub death: Option<u32>,
    /// v39: a zoom window a Damage or Healing drill answers.
    pub range: Option<(u32, u32)>,
    /// v42: the drill's opened ability, by its by-spell row key.
    pub spell: Option<String>,
    /// v42: the pair's second player — `drill` the first.
    pub pair: Option<String>,
    /// v42: build the drill's stacked series.
    pub stacked: bool,
}

impl Ask {
    /// The view's rows and, with `drill`, that player's lists — `death`
    /// picking a Deaths drill's window.
    pub fn of(view: View, drill: Option<&str>, death: Option<u32>) -> Self {
        Self {
            view,
            drill: drill.map(str::to_string),
            death,
            range: None,
            spell: None,
            pair: None,
            stacked: false,
        }
    }
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
    /// The tailer moved off this log to a newer one, so the file is a
    /// finished session: scan it as an OLDER log — its open tail is an
    /// aborted pull and its open visit the night's Σ, both imported the
    /// way the start-up sweep imports them. File-derived on purpose: the
    /// game-process signal never closes anything, so what the live daemon
    /// stores is what a regrade re-reads from the file.
    Retire(PathBuf),
    /// The loader pool finished an import job.
    Loaded {
        job: Box<ImportJob>,
        result: Result<Loaded, String>,
    },
    /// v45 (R29): a session's `GetReplay`, answered `DaemonMsg::Replay`.
    Replay {
        session: u64,
        req_id: u32,
        fight_id: String,
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
        /// A key's member boss (name or index): parsed from the log on
        /// demand through the loader pool, answered when it lands.
        boss: Option<String>,
        /// v42: what is asked of it (boxed: the channel carries every
        /// request at the size of its largest).
        ask: Box<Ask>,
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
    /// Re-derive stored cards from their logs, in place.
    Regrade {
        session: u64,
        req_id: u32,
        fight_id: Option<String>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        kind: Option<FightKind>,
    },
}

impl HistoryReq {
    /// A client read: answerable from the in-memory index, replaceable, and
    /// the only traffic that can arrive faster than the thread drains. These
    /// are the requests [`READ_QUOTA`] bounds.
    fn is_read(&self) -> bool {
        matches!(
            self,
            HistoryReq::Query { .. } | HistoryReq::Fight { .. } | HistoryReq::Replay { .. }
        )
    }
}

/// The hub's handle: a bounded sender plus the status the hub reads
/// synchronously for `Status`. Cloneable so the loader pool can answer
/// import jobs straight back to the thread.
#[derive(Clone)]
pub struct HistoryLink {
    tx: Option<SyncSender<HistoryReq>>,
    status: Arc<Mutex<HistoryStatus>>,
    /// Reads sitting in the channel, unhandled. Raised by `send`, lowered by
    /// the thread once it has taken one off — see [`READ_QUOTA`].
    reads: Arc<AtomicUsize>,
    /// Reads refused for being over the quota. Deliberately NOT
    /// `HistoryStatus::dropped`: that field means "writes the daemon lost",
    /// a client renders it as "your fights may be missing", and a refused
    /// read has lost nothing — the client is answered empty and asks again.
    /// Kept off the wire too; it is a daemon-side pressure gauge.
    refused_reads: Arc<AtomicUsize>,
    /// v35: the account's characters as the thread last resolved them —
    /// what the hub marks every snapshot's rows `mine` from. Swapped whole
    /// (an `Arc` behind the lock), so a read is a refcount, not a copy.
    mine: Arc<Mutex<Arc<Mine>>>,
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
            reads: Arc::new(AtomicUsize::new(0)),
            refused_reads: Arc::new(AtomicUsize::new(0)),
            mine: Arc::default(),
        }
    }

    /// Never blocks: a full queue drops the request, counts it, and hands
    /// it back so a client one-shot can still be answered (empty) by the
    /// caller — the protocol promises every request a reply. A disabled
    /// link swallows silently (the hub never sends to one).
    pub fn send(&self, req: HistoryReq) -> Result<(), HistoryReq> {
        let Some(tx) = &self.tx else { return Ok(()) };
        // A read may only ever hold READ_QUOTA of the queue's slots, so no
        // amount of client polling can starve the `Store` of a closing pull:
        // past the quota the read is refused here, before it takes a slot.
        let read = req.is_read();
        if read && self.reads.fetch_add(1, Ordering::AcqRel) >= READ_QUOTA {
            self.reads.fetch_sub(1, Ordering::AcqRel);
            // Counted apart from `dropped`: nothing was lost, so the store's
            // "writes I dropped" figure must not tick on a client's scrolling.
            self.refused_reads.fetch_add(1, Ordering::Relaxed);
            return Err(req);
        }
        match tx.try_send(req) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(req)) | Err(TrySendError::Disconnected(req)) => {
                if read {
                    self.reads.fetch_sub(1, Ordering::AcqRel);
                    self.refused_reads.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.count_drop();
                }
                Err(req)
            }
        }
    }

    /// A WRITE the daemon lost — the only thing `HistoryStatus::dropped`
    /// has ever meant, and the only thing a client should warn about.
    fn count_drop(&self) {
        if let Ok(mut s) = self.status.lock() {
            s.dropped = s.dropped.saturating_add(1);
        }
    }

    /// The thread's side of the quota: one read has left the channel. Called
    /// before the handler runs — the quota bounds what is WAITING, not how
    /// long an answer takes to compute.
    fn read_done(&self) {
        self.reads.fetch_sub(1, Ordering::AcqRel);
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

    /// v35: the account's characters, as the thread last published them;
    /// nobody while the store is off or before its first publish.
    pub fn mine(&self) -> Arc<Mine> {
        self.mine
            .lock()
            .map(|m| Arc::clone(&m))
            .unwrap_or_else(|e| Arc::clone(&e.into_inner()))
    }

    /// The thread's side: publish a new resolution when it changed.
    fn set_mine(&self, mine: Mine) {
        if let Ok(mut m) = self.mine.lock()
            && **m != mine
        {
            *m = Arc::new(mine);
        }
    }

    /// Reads refused for being over the quota, since start. Not on the wire:
    /// a test and the daemon log are its readers.
    pub fn refused_reads(&self) -> usize {
        self.refused_reads.load(Ordering::Relaxed)
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
            reads: Arc::new(AtomicUsize::new(0)),
            refused_reads: Arc::new(AtomicUsize::new(0)),
            mine: Arc::default(),
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
        reads: Arc::new(AtomicUsize::new(0)),
        refused_reads: Arc::new(AtomicUsize::new(0)),
        mine: Arc::default(),
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
            scans: VecDeque::new(),
            backfill: VecDeque::new(),
            product: opts.addon_dir.clone(),
            addon: None,
            saved_variables: HashMap::new(),
        };
        worker.check_addon();
        worker.poll_saved_variables();
        worker.publish(&status);
        // v42: every fight short of what the store promises it — a kill or
        // a timed key whose details an older build demoted, a series file
        // in an older format — rewritten from its log, newest first, a log's
        // fights at a time while the mailbox is idle (`run`); a log no
        // longer on disk skips its fights. After the first status is out: a
        // details stat per card is not worth a reader's wait.
        worker.backfill.extend(worker.store.rewrites());
        // v45 (R29): and every fight whose replay is wanted and missing (a
        // store older than the tier, a progression wipe): cut alone.
        worker.backfill.extend(worker.store.recuts());
        if let Some(root) = sweep_root {
            worker.sweep(&root);
            worker.publish(&status);
        }
        run(rx, worker, status);
    });
    link
}

/// The thread's loop. A sweep only lists its files; each one is index-
/// scanned here, between messages, so a directory of gigabytes never holds
/// the mailbox shut: a client's `GetHistory` waits for at most one file's
/// scan, not the whole night's.
fn run(rx: Receiver<HistoryReq>, mut w: Worker<DirBackend>, status: Arc<Mutex<HistoryStatus>>) {
    loop {
        let req = if w.scans.is_empty() && w.backfill.is_empty() {
            // Idle: wake now and then to notice a SavedVariables write —
            // the addon's guild data lands on logout, when no fight is
            // closing and no client need be asking.
            match rx.recv_timeout(SAVED_VARIABLES_POLL) {
                Ok(req) => Some(req),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if w.poll_saved_variables() {
                        w.publish(&status);
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
            }
        } else {
            match rx.try_recv() {
                Ok(req) => Some(req),
                Err(TryRecvError::Empty) => {
                    if w.scans.is_empty() {
                        w.backfill_next();
                    } else {
                        w.scan_next();
                    }
                    w.publish(&status);
                    continue;
                }
                Err(TryRecvError::Disconnected) => None,
            }
        };
        let Some(req) = req else { return };
        if req.is_read() {
            w.reply.read_done();
        }
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
    /// Files a sweep listed and `run` has yet to index-scan, with whether
    /// each is the tailed (live) log.
    scans: VecDeque<(PathBuf, bool)>,
    /// v39: fights pinned without the series tier they now earn, waiting
    /// for an idle mailbox to be rewritten from their logs — the pin is
    /// answered at once, and the rescan never holds a read up.
    backfill: VecDeque<String>,
    /// The game's product directory, when the source is inside an install.
    product: Option<PathBuf>,
    /// The addon's installed version after the start-up check.
    addon: Option<String>,
    /// Each account's `wowdps.lua` and the modification time last read,
    /// so a poll costs a stat and a rewrite is read once.
    saved_variables: HashMap<PathBuf, SystemTime>,
}

impl<B: Backend> Worker<B> {
    /// The start-up rule (spec §9a): an installed addon that is out of
    /// date — an older daemon's copy, or a game update that moved the
    /// interface number — is rewritten; one never installed stays missing
    /// (`wowdps addon install` is the user's call). A failed rewrite is the
    /// store's error, not a crash.
    fn check_addon(&mut self) {
        let Some(product) = &self.product else {
            return;
        };
        match addon::ensure_current(product) {
            Ok(state) => self.addon = state.version().map(str::to_string),
            Err(e) => {
                self.addon = addon::inspect(product).version().map(str::to_string);
                self.store.last_error = Some(format!("addon update failed: {e}"));
            }
        }
    }

    /// Read every account's SavedVariables that changed since last time
    /// into the store: the players into `affiliations/`, the NPC
    /// classifications into `creatures.tsv` (one rewrite for every account
    /// read). `true` when an affiliation was merged — the creatures move
    /// nothing `Status` reports.
    fn poll_saved_variables(&mut self) -> bool {
        let Some(product) = self.product.clone() else {
            return false;
        };
        let mut merged = 0usize;
        let mut seen: Vec<Creature> = Vec::new();
        for (account, path) in addon::saved_variables(&product) {
            let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
                continue;
            };
            if self.saved_variables.get(&path) == Some(&modified) {
                continue;
            }
            // Remember the time even when the read fails: a torn file is
            // reread when it changes again, not thirty times a night.
            self.saved_variables.insert(path.clone(), modified);
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    self.store.last_error = Some(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            match addon_table(&text) {
                Ok(Some(data)) => {
                    merged += self
                        .store
                        .merge_affiliations(Affiliation::from_addon_table(&data, &account));
                    seen.extend(Creature::from_addon_table(&data));
                }
                Ok(None) => {}
                Err(e) => {
                    self.store.last_error = Some(format!("{}: {e}", path.display()));
                }
            }
        }
        if !seen.is_empty() {
            self.store.merge_creatures(seen);
        }
        merged > 0
    }

    fn handle(&mut self, req: HistoryReq) {
        match req {
            HistoryReq::Store(fight) => {
                let facts = self.facts(&fight.log.path);
                if let Some(id) = self.store.store(&fight, facts) {
                    // v45 (R29): a live pull's replay is cut from its log as
                    // soon as the mailbox is idle — the meter that closed it
                    // kept no lines.
                    if self.store.wants_recut(&id) && !self.backfill.contains(&id) {
                        self.backfill.push_back(id.clone());
                    }
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
                boss,
                ask,
            } => {
                if let Some(boss) = boss {
                    // Answered when the loader lands it; None right away when
                    // the card, the boss or its log cannot be found.
                    if !self.drill_boss(session, req_id, &fight_id, &boss, *ask) {
                        self.reply_to(
                            session,
                            DaemonMsg::Fight {
                                req_id,
                                fight: None,
                            },
                        );
                    }
                    self.dispatch();
                    return;
                }
                let fight = self
                    .store
                    .stored_fight_in(&self.reply.mine(), &fight_id, &ask);
                self.reply_to(session, DaemonMsg::Fight { req_id, fight });
            }
            HistoryReq::Pin {
                session,
                req_id,
                fight_id,
                pinned,
            } => {
                let pinned = self.store.pin(&fight_id, pinned) && pinned;
                // v39: a pinned fight earns the series tier (v42: and its
                // details); one stored without them (a wipe, or a fight older
                // than the tier) is rewritten from its log, when the log is
                // still on disk — once the mailbox is idle (`backfill_next`),
                // never in this request's turn.
                if pinned
                    && (self.store.wants_rewrite(&fight_id) || self.store.wants_recut(&fight_id))
                    && !self.backfill.contains(&fight_id)
                {
                    self.backfill.push_back(fight_id.clone());
                }
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
                // Answered with the count of LOGS queued for scanning, before
                // any is read: a directory of gigabytes must not hold the
                // client (and every other mailbox message) for its whole scan.
                let before = self.scans.len();
                self.sweep(&path);
                let queued = self.scans.len().saturating_sub(before);
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
            HistoryReq::Regrade {
                session,
                req_id,
                fight_id,
                encounter,
                difficulty,
                kind,
            } => {
                let queued = self.regrade(fight_id.as_deref(), encounter, difficulty, kind);
                self.reply_to(
                    session,
                    DaemonMsg::History {
                        req_id,
                        answer: HistoryAnswer::Regraded { queued },
                    },
                );
                self.dispatch();
            }
            HistoryReq::Index {
                log,
                segments,
                overalls,
            } => {
                self.enqueue_metas(&log, segments, overalls, Vec::new());
                self.dispatch();
            }
            HistoryReq::Sweep(root) => {
                self.sweep(&root);
            }
            HistoryReq::Retire(path) => {
                // Never live: the tailer has already left it. A pending
                // scan of the same file (a start-up sweep that listed it
                // as the newest) is superseded rather than doubled.
                self.scans.retain(|(p, _)| p != &path);
                self.scans.push_back((path, false));
            }
            HistoryReq::Loaded { job, result } => {
                self.inflight = false;
                let id = self.facts(&job.log.path).id;
                self.queued.remove(&fight_id(
                    id,
                    job.meta.start_ms,
                    job.meta.kind == SegmentKind::Overall,
                ));
                if let Some(drill) = job.drill.clone() {
                    let fight = result.ok().and_then(|loaded| {
                        let fight = fight_from_import(&job, loaded.meter.as_deref()?)?;
                        let facts = self.facts(&fight.log.path);
                        Some(self.store.derived_fight_in(
                            &self.reply.mine(),
                            &fight,
                            facts,
                            &drill.ask,
                        ))
                    });
                    self.reply_to(
                        drill.session,
                        DaemonMsg::Fight {
                            req_id: drill.req_id,
                            fight,
                        },
                    );
                    self.dispatch();
                    return;
                }
                match result {
                    Ok(loaded) => {
                        let mut changed = None;
                        if let Some(meter) = loaded.meter.as_deref()
                            && let Some(fight) = fight_from_import(&job, meter)
                        {
                            let facts = self.facts(&fight.log.path);
                            changed = if job.regrade {
                                self.store.regrade(&fight, facts)
                            } else {
                                self.store.store(&fight, facts)
                            };
                        }
                        // v45 (R29): the cut, once its card is in the store
                        // (written just now, or current already).
                        if let Some(cut) = loaded.cut {
                            let id = fight_id(
                                id,
                                job.meta.start_ms,
                                job.meta.kind == SegmentKind::Overall,
                            );
                            if self.store.store_replay(&id, *cut) {
                                changed.get_or_insert(id);
                            }
                        }
                        // One broadcast, once everything the job wrote is
                        // in: a client told of the card can ask for its
                        // replay, and one told nothing (a cut that landed
                        // after its card — a live close, a pin) hears of
                        // the replay that answered `None` before.
                        if let Some(id) = changed {
                            self.changed(id);
                        }
                    }
                    Err(e) => self.store.last_error = Some(e),
                }
                self.dispatch();
            }
            HistoryReq::Replay {
                session,
                req_id,
                fight_id,
            } => {
                // A file past a frame (a whole key's run could be) is no
                // answer the wire can carry: `None`, as if not kept.
                let bytes = self
                    .store
                    .replay_file(&fight_id)
                    .filter(|b| b.len() + fight_id.len() + 64 < wowdps_proto::MAX_FRAME as usize);
                self.reply_to(
                    session,
                    DaemonMsg::Replay {
                        req_id,
                        fight_id,
                        bytes,
                    },
                );
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
        // One resolution of the owner serves both: it walks every card.
        let (store, mine) = self.store.status_and_mine();
        if let Ok(mut s) = status.lock() {
            s.enabled = true;
            s.fights = store.fights;
            s.owner_inferred = store.owner_inferred;
            s.error = store.error;
            s.addon = self.addon.clone();
            s.affiliations = store.affiliations;
            s.affiliations_utc_ms = store.affiliations_utc_ms;
            s.importing = (self.queue.len() + usize::from(self.inflight) + self.scans.len()) as u32;
        }
        // v35: a stored card, a pin or an addon read can each name another
        // of the account's characters. What the hub marks live rows by, and
        // what this thread's own stored answers are marked by.
        self.reply.set_mine(mine);
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
        open: Vec<SegmentMeta>,
    ) {
        let facts = self.facts(&log.path);
        // Keyed visits: their Overall carries the par timers or a "+N"
        // display name (`Visit::display_name`) — an aborted one (open, no
        // END) included, or its member bosses would pass as pulls.
        let keyed: HashSet<u32> = overalls
            .iter()
            .chain(open.iter().filter(|m| m.kind == SegmentKind::Overall))
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
            let id = fight_id(facts.id, meta.start_ms, meta.kind == SegmentKind::Overall);
            if self.store.has(&id) || self.queued.contains(&id) {
                continue;
            }
            self.queued.insert(id);
            // An aborted fight keeps no replay (`Retention::wants_replay`):
            // no cut to make for it.
            let cut = if aborted {
                CutJob::No
            } else {
                CutJob::of(&meta, meta.visit.is_some_and(|v| keyed.contains(&v)))
            };
            self.queue.push_back(ImportJob {
                log: log.clone(),
                meta,
                aborted,
                regrade: false,
                drill: None,
                cut,
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

    /// Queue a rewrite of every selected card from its log: one fight by
    /// id, or every pull of a boss (+ difficulty). The log is found by its
    /// identity among the source's files, the fight by its start in a fresh
    /// (cached) scan, so seeds and byte ranges are exact. Returns how many
    /// were queued; a card whose log is gone is skipped.
    fn regrade(
        &mut self,
        fight_id: Option<&str>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        kind: Option<FightKind>,
    ) -> u32 {
        let picked: Vec<(String, u64, i64, FightKind)> = self
            .store
            .cards()
            .iter()
            .filter(|c| match fight_id {
                Some(id) => c.id == id,
                // Without an id, at least one of encounter / kind must narrow.
                None => {
                    (encounter.is_some() || kind.is_some())
                        && encounter.is_none_or(|e| c.encounter.is_some_and(|x| x.id == e))
                        && difficulty.is_none_or(|d| card_difficulty(c) == Some(d))
                        && kind.is_none_or(|k| c.kind == k)
                }
            })
            .map(|c| (c.id.clone(), c.log, c.start_local_ms, c.kind))
            .collect();
        self.regrade_picked(picked)
    }

    /// v42: the backfill's batch — one log's fights at a time
    /// (`backfill_next`): each card named in `full` rewritten from its log,
    /// and (v45) each named in `cut_only` cut alone, its card current and its
    /// replay not — one scan of the log for both.
    fn backfill_ids(
        &mut self,
        full: &HashSet<String>,
        cut_only: &HashSet<String>,
        slots: &HashSet<String>,
    ) -> u32 {
        let picked = self
            .store
            .cards()
            .iter()
            .filter(|c| full.contains(&c.id) || cut_only.contains(&c.id))
            .map(|c| (c.id.clone(), c.log, c.start_local_ms, c.kind))
            .collect();
        self.queue_picked(picked, cut_only, slots)
    }

    /// Queue the rewrite of each picked card (id, log, start, kind).
    fn regrade_picked(&mut self, picked: Vec<(String, u64, i64, FightKind)>) -> u32 {
        let slots = self.store.replay_slots();
        self.queue_picked(picked, &HashSet::new(), &slots)
    }

    /// Queue a job per picked card: its rewrite — the replay cut beside it
    /// when the fight holds one of the replay `slots` (v45) — or, for an id
    /// in `cut_only`, the cut alone.
    fn queue_picked(
        &mut self,
        picked: Vec<(String, u64, i64, FightKind)>,
        cut_only: &HashSet<String>,
        slots: &HashSet<String>,
    ) -> u32 {
        // One scan per LOG, not per card — `--kind encounter` picks hundreds
        // of cards out of a few dozen logs — and the logs side by side: the
        // requester's answer (and every other mailbox message) waits on them
        // all, and most are full rescans, the index cache keeping eight.
        let mut paths: HashMap<u64, Option<PathBuf>> = HashMap::new();
        for (id, log, ..) in &picked {
            if !self.queued.contains(id) {
                paths.entry(*log).or_insert_with(|| self.path_of_log(*log));
            }
        }
        let logs = paths
            .into_iter()
            .filter_map(|(log, path)| Some((log, path?)))
            .collect();
        let scanned = scan_logs(self.cache.as_ref(), logs);
        let mut queued = 0;
        for (id, log, start_ms, card_kind) in picked {
            if self.queued.contains(&id) {
                continue;
            }
            let Some((path, idx)) = scanned.get(&log) else {
                continue;
            };
            // A visit and its first segment can start on the same line: a Σ
            // card matches Overall metas only, a pull matches segments only.
            let is_sigma = matches!(card_kind, FightKind::Key | FightKind::Overall);
            let closed = if is_sigma {
                idx.overalls.iter().find(|m| m.start_ms == start_ms)
            } else {
                idx.segments.iter().find(|m| m.start_ms == start_ms)
            }
            .map(|m| (m.clone(), false));
            let open = if is_sigma {
                idx.open_visit
                    .iter()
                    .map(|v| {
                        let keyed = v.pars_ms.is_some() || looks_keyed(&v.name);
                        (v.clone(), keyed && v.success.is_none())
                    })
                    .find(|(m, _)| m.start_ms == start_ms)
            } else {
                idx.open
                    .iter()
                    .map(|m| (m.clone(), true))
                    .find(|(m, _)| m.start_ms == start_ms)
            };
            let Some((meta, aborted)) = closed.or(open) else {
                continue;
            };
            let only_cut = cut_only.contains(&id);
            let cut = if only_cut {
                CutJob::Only
            } else if slots.contains(&id)
                && self
                    .store
                    .card(&id)
                    .is_some_and(|c| self.store.cfg.wants_replay(c))
            {
                CutJob::Also
            } else {
                CutJob::No
            };
            self.queued.insert(id);
            self.queue.push_back(ImportJob {
                log: LogRef { path: path.clone() },
                meta,
                aborted,
                regrade: !only_cut,
                drill: None,
                cut,
            });
            queued += 1;
        }
        queued
    }

    /// Queue a parse of one of a key's member bosses — `boss` a name
    /// (case-insensitive) or a 0-based index into the card's `bosses` — so
    /// the requester gets the boss's own rows / breakdown. `false` when the
    /// card, the boss, or its log cannot be found (the caller answers None).
    fn drill_boss(
        &mut self,
        session: u64,
        req_id: u32,
        fight_id: &str,
        boss: &str,
        ask: Ask,
    ) -> bool {
        let Some(card) = self.store.card(fight_id).cloned() else {
            return false;
        };
        let want = boss.to_lowercase();
        let member = match want.parse::<usize>() {
            Ok(i) => card.bosses.get(i),
            Err(_) => card.bosses.iter().find(|b| b.name.to_lowercase() == want),
        };
        let Some(member) = member else {
            return false;
        };
        let start_local = member.start_utc_ms + i64::from(card.tz_min.unwrap_or(0)) * 60_000;
        let Some((path, idx)) = self.scan_log(card.log) else {
            return false;
        };
        let Some(meta) = idx
            .segments
            .iter()
            .find(|m| m.start_ms == start_local && m.kind == SegmentKind::Encounter)
            .cloned()
        else {
            return false;
        };
        // Drills never dedup against imports: a different consumer.
        self.queue.push_back(ImportJob {
            log: LogRef { path },
            meta,
            aborted: false,
            regrade: false,
            drill: Some(DrillReq {
                session,
                req_id,
                ask,
            }),
            cut: CutJob::No,
        });
        true
    }
    fn scan(&self, path: &Path) -> Option<index::Index> {
        scan_path(self.cache.as_ref(), path)
    }

    /// The log whose header hashes to `log`, index-scanned.
    fn scan_log(&mut self, log: u64) -> Option<(PathBuf, index::Index)> {
        let path = self.path_of_log(log)?;
        let idx = self.scan(&path)?;
        Some((path, idx))
    }

    /// The file whose header hashes to `log`: the daemon's own source (a
    /// file, or every log in its directory), plus any path already seen.
    fn path_of_log(&mut self, log: u64) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = self.logs.keys().cloned().collect();
        match &self.source {
            Some(SourceSpec::File(p)) => candidates.push(p.clone()),
            Some(SourceSpec::Dir(d)) => {
                if let Ok(rd) = std::fs::read_dir(d) {
                    candidates.extend(rd.flatten().map(|e| e.path()).filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("WoWCombatLog") && n.ends_with(".txt"))
                    }));
                }
            }
            None => {}
        }
        candidates.into_iter().find(|p| self.facts(p).id == log)
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
        // "Is" means the same file, not the same spelling: the import path
        // is canonicalized by the CLI, the config's `logs_dir` is not.
        let newest = match &self.source {
            Some(SourceSpec::File(p)) => Some(p.clone()),
            Some(SourceSpec::Dir(d)) => newest_log(d),
            None if root.is_dir() => newest_log(root),
            None => Some(root.to_path_buf()),
        };
        for path in files {
            let live = newest.as_deref().is_some_and(|n| same_file(n, &path));
            // Listed only: `run` scans one file at a time between messages.
            if !self.scans.iter().any(|(p, _)| p == &path) {
                self.scans.push_back((path, live));
            }
        }
    }

    /// Index-scan the next swept file and queue what the store lacks.
    /// v39: one fight's rewrite from its log, when it still wants one (a
    /// pin let go since, or a regrade that got there first, leaves nothing
    /// to do) — v42: a pinned fight, or one start-up found short of what
    /// the store promises it (`Store::rewrites`).
    /// v42: the next fight's log at a time — every queued fight of that
    /// log in one batch, so the log is scanned once (as `regrade` does a
    /// boss's pulls), not once per fight, while a reader waits on one log
    /// at most.
    fn backfill_next(&mut self) {
        let Some(first) = self.backfill.pop_front() else {
            return;
        };
        let log = self.store.card(&first).map(|c| c.log);
        let mut batch: HashSet<String> = HashSet::from([first]);
        self.backfill.retain(|id| {
            let same = log.is_some() && self.store.card(id).map(|c| c.log) == log;
            if same {
                batch.insert(id.clone());
            }
            !same
        });
        // A fight short of its tiers is rewritten whole (its replay cut
        // beside it, v45); one whose card is current but whose replay is
        // not gets the cut alone. Each set is asked of the store once.
        let full: HashSet<String> = batch
            .iter()
            .filter(|id| self.store.wants_rewrite(id))
            .cloned()
            .collect();
        let protected = self.store.protected();
        let slots = self.store.slots_from(&protected);
        let recut: HashSet<String> = batch
            .iter()
            .filter(|id| !full.contains(*id))
            .filter(|id| self.store.wants_replay_rewrite(id, &slots, &protected))
            .cloned()
            .collect();
        if !full.is_empty() || !recut.is_empty() {
            self.backfill_ids(&full, &recut, &slots);
            self.dispatch();
        }
    }

    fn scan_next(&mut self) {
        let Some((path, live)) = self.scans.pop_front() else {
            return;
        };
        let Some(idx) = self.scan(&path) else {
            return;
        };
        // The tailed log's open tail is live, not aborted. Anything still
        // open in an older log never closes — including its last VISIT:
        // zoning out only suspends a visit (R10), so the raid itself is
        // still open at EOF and its Σ exists only as `open_visit`. A keyed
        // run whose END fired is a finished run (not aborted); a key
        // without one is; a plain visit's Σ merges only closed members and
        // is stored as is. Since R10's END became terminal a finished key
        // closes on its own — this arm is the belt to that braces, and
        // still the only path for an older log's abandoned key or raid.
        let (open, overalls) = if live {
            (Vec::new(), idx.overalls)
        } else {
            let mut overalls = idx.overalls;
            let mut open: Vec<SegmentMeta> = idx.open.clone().into_iter().collect();
            if let Some(v) = idx.open_visit.clone() {
                let keyed = v.pars_ms.is_some() || looks_keyed(&v.name);
                if keyed && v.success.is_none() {
                    open.push(v);
                } else {
                    overalls.push(v);
                }
            }
            (open, overalls)
        };
        self.enqueue_metas(&LogRef { path }, idx.segments, overalls, open);
        self.dispatch();
    }
}

/// The same file under two spellings (a symlinked `logs_dir`, a relative
/// path, a canonicalized import argument): compared canonically when both
/// resolve, textually otherwise.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Index-scan one file, resumed from the cache's checkpoint when it has one.
fn scan_path(cache: Option<&IndexCache>, path: &Path) -> Option<index::Index> {
    let mut file = std::fs::File::open(path).ok()?;
    Some(match cache {
        Some(cache) => cache.scan_file(path, &mut file),
        None => index::scan(&mut file),
    })
}

/// Index-scan several logs side by side, keyed by log id: each thread takes
/// the next log, on half the machine's threads at most so a regrade leaves a
/// running game its share. Safe on one cache: a checkpoint lands by atomic
/// rename, and the engine's tailer shares the directory already.
fn scan_logs(
    cache: Option<&IndexCache>,
    logs: Vec<(u64, PathBuf)>,
) -> HashMap<u64, (PathBuf, index::Index)> {
    let threads = thread::available_parallelism()
        .map_or(1, |n| (n.get() / 2).max(1))
        .min(logs.len());
    let next = AtomicUsize::new(0);
    let out = Mutex::new(HashMap::new());
    thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                while let Some((log, path)) = logs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(idx) = scan_path(cache, path) {
                        out.lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(*log, (path.clone(), idx));
                    }
                }
            });
        }
    });
    out.into_inner().unwrap_or_else(|e| e.into_inner())
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

/// The boss pulls of visit `ord` as a key's card lists them: Encounter
/// segments in pull order, start in LOG-LOCAL ms (the tz shift happens
/// when the card is written).
pub fn visit_members(meter: &Meter, ord: u32) -> Vec<KeyBoss> {
    meter
        .segments()
        .iter()
        .filter(|s| s.visit == Some(ord) && s.kind == SegmentKind::Encounter)
        .map(|s| KeyBoss {
            name: s.name.clone(),
            encounter: s.encounter,
            start_utc_ms: s.start_ms,
            duration_ms: s.duration_ms(s.last_combat_ms()),
            success: s.success,
        })
        .collect()
}
/// Reassemble a `ClosedFight` from an import job's parsed slice — the same
/// meter the engine would show for that segment.
fn fight_from_import(job: &ImportJob, meter: &Meter) -> Option<ClosedFight> {
    let mut members = Vec::new();
    let (segment, visit) = match job.meta.kind {
        SegmentKind::Overall => {
            let ord = job.meta.visit?;
            let mut seg = meter.overall(ord)?;
            let visit = meter.visits().get(ord as usize).cloned();
            // The verdict the live path stamps in take_closed: a key's
            // timed / over-time, from its END against the par timers.
            if let Some(v) = &visit {
                seg.success = v.verdict(seg.last_combat_ms());
            }
            members = visit_members(meter, ord);
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
        members,
    })
}

// ---- backends -------------------------------------------------------------------

/// Where the documents live. Directory in production, memory for the mock
/// and the tests. `dir` is one of `fights`, `rows`, `details`, `loadouts`,
/// `annotations`; `name` is the file name.
pub trait Backend {
    fn list(&self, dir: &str) -> Vec<String>;
    fn read(&self, dir: &str, name: &str) -> Option<Vec<u8>>;
    /// v39: `len` bytes at `offset` — what the series tier's reader asks
    /// for (its head, its index, one player's block). `None` past the end.
    fn read_range(&self, dir: &str, name: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        let all = self.read(dir, name)?;
        let at = usize::try_from(offset).ok()?;
        all.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
    }
    /// v45: a file's size in bytes — what the replay tier's cap counts.
    fn size(&self, dir: &str, name: &str) -> Option<u64> {
        self.read(dir, name).map(|b| b.len() as u64)
    }
    fn exists(&self, dir: &str, name: &str) -> bool;
    fn write(&mut self, dir: &str, name: &str, bytes: &[u8]) -> io::Result<()>;
    fn remove(&mut self, dir: &str, name: &str) -> io::Result<()>;
}

/// `len` bytes of a file at `offset`, read alone. The length comes from a
/// file's own index: never trusted past the file's end, so a torn file
/// costs a `None`, not an allocation.
fn read_file_range(path: &Path, offset: u64, len: usize) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    if offset.checked_add(len as u64)? > size {
        return None;
    }
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
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

    fn size(&self, dir: &str, name: &str) -> Option<u64> {
        std::fs::metadata(self.root.join(dir).join(name))
            .ok()
            .map(|m| m.len())
    }

    fn read_range(&self, dir: &str, name: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        read_file_range(&self.root.join(dir).join(name), offset, len)
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
    /// A real store read through, never written (`over_dir`): a file not
    /// in `files` is read from here unless `removed` hides it.
    seed: Option<PathBuf>,
    /// Seed files removed in memory — retention's demotions over a seeded
    /// store land here, never on the disk.
    removed: std::collections::BTreeSet<(String, String)>,
}

impl MemBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store in memory over a real one on disk, READ-ONLY: every read
    /// falls through to `root` (`$XDG_DATA_HOME/wowdps/history/v1` or a
    /// copy), every write and remove — the store's own migrations and
    /// retention included — stays in memory. What lets the mock answer
    /// history from a real machine's cards without ever opening a
    /// `DirBackend` on them.
    pub fn over_dir(root: &Path) -> Self {
        Self {
            seed: Some(root.to_path_buf()),
            ..Self::default()
        }
    }

    /// The files written in memory — a seed's files are not counted.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn key(dir: &str, name: &str) -> (String, String) {
        (dir.to_string(), name.to_string())
    }

    /// A seed file's path, unless it was removed in memory.
    fn seeded(&self, dir: &str, name: &str) -> Option<PathBuf> {
        let root = self.seed.as_ref()?;
        (!self.removed.contains(&Self::key(dir, name))).then(|| root.join(dir).join(name))
    }
}

impl Backend for MemBackend {
    fn list(&self, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .keys()
            .filter(|(d, _)| d == dir)
            .map(|(_, n)| n.clone())
            .collect();
        if let Some(root) = &self.seed {
            // `DirBackend::list`'s rule: a `.tmp` is a write in flight.
            let seeded = std::fs::read_dir(root.join(dir))
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.ends_with(".tmp") && self.seeded(dir, n).is_some());
            names.extend(seeded);
            names.sort();
            names.dedup();
        }
        names
    }

    fn read(&self, dir: &str, name: &str) -> Option<Vec<u8>> {
        match self.files.get(&Self::key(dir, name)) {
            Some(bytes) => Some(bytes.clone()),
            None => std::fs::read(self.seeded(dir, name)?).ok(),
        }
    }

    // A size and a range from what is in hand: a read-through store's
    // open learns every replay's size and head, and must not read a
    // season's files whole to do it.
    fn size(&self, dir: &str, name: &str) -> Option<u64> {
        match self.files.get(&Self::key(dir, name)) {
            Some(bytes) => Some(bytes.len() as u64),
            None => std::fs::metadata(self.seeded(dir, name)?)
                .ok()
                .map(|m| m.len()),
        }
    }

    fn read_range(&self, dir: &str, name: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        match self.files.get(&Self::key(dir, name)) {
            Some(bytes) => {
                let at = usize::try_from(offset).ok()?;
                bytes.get(at..at.checked_add(len)?).map(<[u8]>::to_vec)
            }
            None => read_file_range(&self.seeded(dir, name)?, offset, len),
        }
    }

    fn exists(&self, dir: &str, name: &str) -> bool {
        self.files.contains_key(&Self::key(dir, name))
            || self.seeded(dir, name).is_some_and(|p| p.exists())
    }

    fn write(&mut self, dir: &str, name: &str, bytes: &[u8]) -> io::Result<()> {
        if self.fail_writes {
            return Err(io::Error::other("simulated write failure"));
        }
        self.removed.remove(&Self::key(dir, name));
        self.files.insert(Self::key(dir, name), bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, dir: &str, name: &str) -> io::Result<()> {
        self.files.remove(&Self::key(dir, name));
        if self.seed.is_some() {
            self.removed.insert(Self::key(dir, name));
        }
        Ok(())
    }
}

// ---- the store --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retention {
    pub store_trash: bool,
    pub keep_per_encounter: usize,
    pub keep_details_per_encounter: usize,
    /// Details are written for kills and for wipes lasting at least this
    /// many seconds (`history_details_min_wipe_secs`); never for aborted
    /// fights or shorter wipes.
    pub details_min_wipe_secs: u64,
    /// v42: `history_keep_kills_whole` — every boss kill and timed key is
    /// in the protected set ([`Retention::keeps_whole`]).
    pub keep_whole: bool,
    /// v45: `history_keep_progression_whole` — every wipe on a boss the
    /// store has not seen killed at that difficulty is kept whole too
    /// ([`Store::is_progression`]).
    pub keep_progression: bool,
    /// v45 (R29): `history_replay_mb`, in bytes — the replay tier's cap over
    /// the fights retention may touch; 0 = none.
    pub replay_bytes: u64,
    pub characters: Vec<String>,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            store_trash: false,
            keep_per_encounter: 200,
            keep_details_per_encounter: 10,
            details_min_wipe_secs: 60,
            keep_whole: true,
            keep_progression: true,
            replay_bytes: 4096 << 20,
            characters: Vec::new(),
        }
    }
}

impl Retention {
    /// Whether a fight earns the details tier at write time: a kill, or a
    /// wipe (not aborted) lasting at least `details_min_wipe_secs`.
    /// Retention may still demote it later; a reader tells "demoted" from
    /// "never written" by applying this same rule to the card.
    pub fn wants_details(&self, card: &FightCard) -> bool {
        match card.success {
            Some(true) => true,
            Some(false) => {
                !card.aborted
                    && card.duration_ms
                        >= i64::try_from(self.details_min_wipe_secs)
                            .unwrap_or(i64::MAX)
                            .saturating_mul(1000)
            }
            None => false,
        }
    }

    /// v39: whether a fight keeps the SERIES tier — its abilities and
    /// targets second by second, what lets a stored drill answer a zoom
    /// window: a kill, a key (timed or not) or a pinned fight, and only
    /// while it earns details (a window of lists the store no longer
    /// keeps would answer nothing). A wipe gains it when it is pinned,
    /// through a rewrite from its log; unpinned, it loses it again.
    pub fn wants_series(&self, card: &FightCard) -> bool {
        self.wants_details(card)
            && (card.success == Some(true) || card.kind == FightKind::Key || card.pinned)
    }

    /// v42: the fights the store keeps WHOLE for as long as it keeps
    /// anything — a boss kill and a timed key. Retention never demotes or
    /// evicts them (the protected set), so their details and series tiers
    /// — the stacked abilities, an opened ability, a comparison — last the
    /// season; a pin keeps any other fight the same way. Wipes, trash,
    /// arenas and over-time keys answer to the per-group caps. (A season's
    /// archive is a later feature.) `history_keep_kills_whole = false` turns
    /// it off, and the caps count them again.
    pub fn keeps_whole(&self, card: &FightCard) -> bool {
        self.keep_whole
            && card.success == Some(true)
            && !card.aborted
            && matches!(card.kind, FightKind::Encounter | FightKind::Key)
    }

    /// v45 (R29): whether a fight is cut into the REPLAY tier at all — every
    /// boss pull and keystone run, wipes included; never trash, an arena, a
    /// raid night's Σ or an aborted fight (a pull the log abandoned, which
    /// would read as a wipe at 0:00 — the details rule leaves it out too).
    /// Which of them keep it is retention's ([`Store::replay_slots`]).
    pub fn wants_replay(&self, card: &FightCard) -> bool {
        matches!(card.kind, FightKind::Encounter | FightKind::Key) && !card.aborted
    }
}

impl From<&HistoryOptions> for Retention {
    fn from(o: &HistoryOptions) -> Self {
        Self {
            store_trash: o.store_trash,
            keep_per_encounter: o.keep_per_encounter,
            keep_details_per_encounter: o.keep_details_per_encounter,
            details_min_wipe_secs: o.details_min_wipe_secs,
            keep_whole: o.keep_kills_whole,
            keep_progression: o.keep_progression_whole,
            replay_bytes: o.replay_mb.saturating_mul(1 << 20),
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
    /// `affiliations/<guid>.json`, by guid: what the wowdps addon last saw
    /// of each player (spec §9a). Joined onto cards when they are answered.
    affiliations: HashMap<String, Affiliation>,
    /// `creatures.tsv` at the store's root, by creature id: the NPC
    /// classifications the wowdps addon saw (`proto::creatures`), read at
    /// open and rewritten whole when a merge changes a row.
    creatures: BTreeMap<u32, Creature>,
    /// Why the `creatures.tsv` on disk is one this build must not rewrite
    /// (a newer format, a file not ours); `None` when it can.
    creatures_refused: Option<String>,
    /// v39: the fights whose `series/<id>.bin` is on disk, listed once at
    /// open and kept with every write and removal — so retention never
    /// stats a file per card to know — with (v42) the format its head names
    /// (`None`: no series head), read once at open, so what the tier can
    /// answer and whether it wants rewriting are known without a read.
    series: HashMap<String, Option<u8>>,
    /// v45 (R29): the fights whose `replay/<id>.bin` is on disk, with the
    /// format its head names and its size, listed once at open and kept
    /// with every write and removal, as `series` is.
    replays: HashMap<String, (Option<u8>, u64)>,
    /// v45: every (encounter id, difficulty) a stored card KILLED — what
    /// tells a progression wipe (`Store::is_progression`) from an ordinary
    /// one. Rebuilt whenever the cards change.
    killed: HashSet<(u32, u32)>,
}

impl<B: Backend> Store<B> {
    /// Rebuild the index from `fights/`. Unreadable cards are skipped and
    /// counted; the rest are served.
    pub fn open(mut backend: B, cfg: Retention) -> Self {
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
        // Σ cards written before the id carried its mark sit under the
        // pull spelling, where a member starting on the visit's millisecond
        // collides with them. Move each to its marked id, all tiers along.
        for card in cards.iter_mut() {
            let sigma = matches!(card.kind, FightKind::Key | FightKind::Overall);
            let marked = sigma_id(&card.id);
            if !sigma || marked == card.id {
                continue;
            }
            let old = std::mem::replace(&mut card.id, marked.clone());
            for (dir, ext) in [
                ("rows", "json"),
                ("details", "json"),
                ("series", "bin"),
                ("replay", "bin"),
                ("annotations", "ndjson"),
            ] {
                let from = format!("{old}.{ext}");
                if let Some(bytes) = backend.read(dir, &from)
                    && backend
                        .write(dir, &format!("{marked}.{ext}"), &bytes)
                        .is_ok()
                {
                    let _ = backend.remove(dir, &from);
                }
            }
            if backend
                .write(
                    "fights",
                    &format!("{marked}.json"),
                    card.to_json().to_line().as_bytes(),
                )
                .is_ok()
            {
                let _ = backend.remove("fights", &format!("{old}.json"));
            }
        }
        cards.sort_by_key(|c| c.start_utc_ms);
        let last_error =
            (corrupt > 0).then(|| format!("{corrupt} unreadable card(s) in fights/ skipped"));
        // Affiliations are small and few (one per player ever raided with);
        // an unreadable one is skipped like a card, uncounted — the addon
        // rewrites it on the next logout anyway.
        let affiliations = backend
            .list("affiliations")
            .into_iter()
            .filter_map(|name| {
                let bytes = backend.read("affiliations", &name)?;
                let text = String::from_utf8(bytes).ok()?;
                Affiliation::from_json(&json::parse(&text).ok()?)
            })
            .map(|a| (a.guid.clone(), a))
            .collect();
        // The addon's NPC classifications, one file at the root. A file
        // this build cannot read whole is left as it is, never rewritten.
        let (creatures, creatures_refused) = match backend.read("", creatures::FILE) {
            None => (BTreeMap::new(), None),
            Some(bytes) => match String::from_utf8(bytes)
                .map_err(|_| format!("{}: not UTF-8", creatures::FILE))
                .and_then(|text| creatures::parse(&text))
            {
                Ok(rows) => (rows, None),
                Err(e) => (BTreeMap::new(), Some(e)),
            },
        };
        let last_error = last_error.or_else(|| creatures_refused.clone());
        // v39: which fights keep the series tier, by file name; v42: and in
        // what format, off each file's nine-byte head.
        let series = backend
            .list("series")
            .into_iter()
            .filter_map(|name| {
                let id = name.strip_suffix(".bin")?.to_string();
                let format = backend
                    .read_range("series", &name, 0, series_tier::HEAD_LEN)
                    .and_then(|head| series_tier::format_of(&head));
                Some((id, format))
            })
            .collect();
        // v45 (R29): which fights keep the replay tier, its format and size.
        let replays = backend
            .list("replay")
            .into_iter()
            .filter_map(|name| {
                let id = name.strip_suffix(".bin")?.to_string();
                let format = backend
                    .read_range("replay", &name, 0, replay_tier::HEAD_LEN)
                    .and_then(|head| replay_tier::format_of(&head));
                let size = backend.size("replay", &name).unwrap_or(0);
                Some((id, (format, size)))
            })
            .collect();
        let mut store = Self {
            backend,
            cfg,
            cards,
            last_error,
            corrupt,
            affiliations,
            creatures,
            creatures_refused,
            series,
            replays,
            killed: HashSet::new(),
        };
        store.refresh_killed();
        // Cards stamped with the store-wide owner before ownership was
        // per-card (or before the addon named the alt) get their own.
        store.repair_owners();
        store
    }

    // ---- v45: progression and the replay tier ---------------------------------

    /// Rebuild [`Store::killed`] from the cards.
    fn refresh_killed(&mut self) {
        self.killed = self
            .cards
            .iter()
            .filter(|c| c.kind == FightKind::Encounter && c.success == Some(true) && !c.aborted)
            .filter_map(|c| c.encounter.map(|e| (e.id, e.difficulty)))
            .collect();
    }

    /// v45: a PROGRESSION wipe — a boss pull lost (not aborted) at a
    /// difficulty no card the store holds has killed it at. Kept whole as a
    /// kill is (details, series, replay; protected from the caps) while
    /// `history_keep_progression_whole` holds, so every pull of a
    /// progression is there to compare; once the first kill there lands,
    /// the wipes before it answer to the caps again (pins still protect). A
    /// Heroic kill says nothing of a Mythic wipe.
    pub fn is_progression(&self, card: &FightCard) -> bool {
        self.cfg.keep_progression
            && card.kind == FightKind::Encounter
            && card.success == Some(false)
            && !card.aborted
            && card
                .encounter
                .is_some_and(|e| !self.killed.contains(&(e.id, e.difficulty)))
    }

    /// The details rule with progression: a kill, a long enough wipe, or
    /// (v45) a progression wipe of any length.
    pub fn earns_details(&self, card: &FightCard) -> bool {
        self.cfg.wants_details(card) || self.is_progression(card)
    }

    /// The series rule with progression (v45): a progression wipe keeps
    /// the series tier as a kill does.
    pub fn earns_series(&self, card: &FightCard) -> bool {
        self.cfg.wants_series(card) || self.is_progression(card)
    }

    /// Kept whole: a kill or timed key ([`Retention::keeps_whole`]) or (v45)
    /// a progression wipe.
    pub fn kept_whole(&self, card: &FightCard) -> bool {
        self.cfg.keeps_whole(card) || self.is_progression(card)
    }

    /// v45 (R29): the fights that keep a replay — every one retention keeps
    /// whole or protects (kills, timed keys, progression wipes, pins, the
    /// fastest kill and the owner's bests, the annotated), and per group
    /// the newest `keep_details_per_encounter` of the rest: the details
    /// tier's own caps. The size cap (`history_replay_mb`) trims the rest
    /// further at write time ([`Store::retain`]).
    pub fn replay_slots(&self) -> HashSet<String> {
        self.slots_from(&self.protected())
    }

    /// [`Store::replay_slots`] over a protected set in hand.
    fn slots_from(&self, protected: &HashSet<String>) -> HashSet<String> {
        let mut groups: BTreeMap<(u8, u32, u32), Vec<&FightCard>> = BTreeMap::new();
        for c in self.cards.iter().filter(|c| self.cfg.wants_replay(c)) {
            groups.entry(group_key(c)).or_default().push(c);
        }
        let mut out: HashSet<String> = HashSet::new();
        for cards in groups.values() {
            let rest: Vec<&&FightCard> = cards
                .iter()
                .filter(|c| !protected.contains(&c.id))
                .collect();
            let skip = rest
                .len()
                .saturating_sub(self.cfg.keep_details_per_encounter);
            out.extend(rest.into_iter().skip(skip).map(|c| c.id.clone()));
            out.extend(
                cards
                    .iter()
                    .filter(|c| protected.contains(&c.id))
                    .map(|c| c.id.clone()),
            );
        }
        out
    }

    /// v45: the fight keeps the replay tier — a file this build reads.
    pub fn has_replay(&self, id: &str) -> bool {
        self.replays.get(id).is_some_and(|(f, _)| {
            f.is_some_and(|f| (replay_tier::OLDEST..=replay_tier::FORMAT).contains(&f))
        })
    }

    /// v45: the replay file's bytes, as `GetReplay` answers them.
    pub fn replay_file(&self, id: &str) -> Option<Vec<u8>> {
        if !self.has_replay(id) {
            return None;
        }
        self.backend.read("replay", &format!("{id}.bin"))
    }

    /// v45: the replay tier's bytes on disk.
    pub fn replay_total(&self) -> u64 {
        self.replays.values().map(|(_, size)| size).sum()
    }

    /// v45: a fight that wants its replay (re)cut from its log: a boss pull
    /// or a run in a replay slot whose file is missing, has no replay head
    /// or is older than this build's (never a newer one) — and, at the size
    /// cap, only one retention protects or one the trim would keep (newer
    /// than the oldest unprotected replay held).
    pub fn wants_replay_rewrite(
        &self,
        id: &str,
        slots: &HashSet<String>,
        protected: &HashSet<String>,
    ) -> bool {
        let Some(card) = self.card(id) else {
            return false;
        };
        if !self.cfg.wants_replay(card) || !slots.contains(id) {
            return false;
        }
        let stale = self
            .replays
            .get(id)
            .is_none_or(|(f, _)| f.is_none_or(|f| f < replay_tier::FORMAT));
        if !stale {
            return false;
        }
        let cap = self.cfg.replay_bytes;
        if cap == 0 || protected.contains(id) || self.replay_total() < cap {
            return true;
        }
        // At or past the cap, as the trim counts it (oldest unprotected
        // first): a fight newer than the oldest unprotected replay held is
        // cut, and the trim takes that one in its place — so a store full to
        // the byte still takes its newest wipes — while an older one would
        // only be cut to be trimmed again.
        let pos = |id: &str| self.cards.iter().position(|c| c.id == id);
        let oldest = self
            .cards
            .iter()
            .position(|c| self.replays.contains_key(&c.id) && !protected.contains(&c.id));
        oldest.is_some_and(|o| pos(id).is_some_and(|p| p > o))
    }

    /// [`Store::wants_replay_rewrite`] for one fight, its sets made here.
    pub fn wants_recut(&self, id: &str) -> bool {
        let protected = self.protected();
        let slots = self.slots_from(&protected);
        self.wants_replay_rewrite(id, &slots, &protected)
    }

    /// v45 (R29): keep `cut` as the fight's replay tier, its owner the
    /// card's, when the fight earns a replay slot; then the size cap may
    /// take the oldest unprotected replay. Adding a replay moves no card and
    /// no slot, so the size cap is all it can break: one walk of the
    /// protected set serves the slot test and the trim, never a whole
    /// retention pass (an import's `store` has just run one). `false` when
    /// the card is unknown, earns none, or the write fails.
    pub fn store_replay(&mut self, id: &str, mut cut: Cut) -> bool {
        let Some(card) = self.card(id) else {
            return false;
        };
        if !self.cfg.wants_replay(card) {
            return false;
        }
        let owner = card.owner.clone();
        let protected = self.protected();
        if !self.slots_from(&protected).contains(id) {
            return false;
        }
        cut.set_owner(owner.as_deref());
        let bytes = replay_tier::encode(&cut);
        if let Err(e) = self.backend.write("replay", &format!("{id}.bin"), &bytes) {
            self.last_error = Some(format!("history write failed: {e}"));
            return false;
        }
        self.replays.insert(
            id.to_string(),
            (Some(replay_tier::FORMAT), bytes.len() as u64),
        );
        self.trim_replays(&protected);
        true
    }

    /// v45: past `history_replay_mb`, the oldest unprotected replays go
    /// until the tier is back under it — never a protected one.
    fn trim_replays(&mut self, protected: &HashSet<String>) {
        if self.cfg.replay_bytes == 0 {
            return;
        }
        let mut total = self.replay_total();
        if total <= self.cfg.replay_bytes {
            return;
        }
        // Oldest first: the cards are sorted by start.
        let oldest: Vec<String> = self
            .cards
            .iter()
            .filter(|c| self.replays.contains_key(&c.id) && !protected.contains(&c.id))
            .map(|c| c.id.clone())
            .collect();
        for id in oldest {
            if total <= self.cfg.replay_bytes {
                break;
            }
            total = total.saturating_sub(self.replays.get(&id).map_or(0, |r| r.1));
            self.drop_replay(&id);
        }
    }

    /// v45: remove a fight's replay tier, if it has one.
    fn drop_replay(&mut self, id: &str) {
        if self.replays.remove(id).is_some() {
            let _ = self.backend.remove("replay", &format!("{id}.bin"));
        }
    }

    /// v45: the kept-whole set by why, each card once: (kills, timed keys,
    /// progression wipes, pins).
    pub fn kept_counts(&self) -> (u32, u32, u32, u32) {
        let mut out = (0, 0, 0, 0);
        for c in &self.cards {
            if self.cfg.keeps_whole(c) && c.kind == FightKind::Encounter {
                out.0 += 1;
            } else if self.cfg.keeps_whole(c) {
                out.1 += 1;
            } else if self.is_progression(c) {
                out.2 += 1;
            } else if c.pinned {
                out.3 += 1;
            }
        }
        out
    }

    // ---- affiliations (spec §9a) --------------------------------------------

    /// Take the addon's records in: a guid's record is written when the
    /// store has none, or when this one was SEEN later (an account's file
    /// is rewritten whole on every logout, so most of it is old news). The
    /// number written. A record the addon saw earlier than the stored one
    /// — another account's older file — changes nothing.
    pub fn merge_affiliations(&mut self, recs: Vec<Affiliation>) -> usize {
        let mut written = 0;
        for rec in recs {
            let newer = match self.affiliations.get(&rec.guid) {
                None => true,
                Some(have) => {
                    rec.seen_utc_ms > have.seen_utc_ms
                        || (rec.seen_utc_ms == have.seen_utc_ms && rec != *have)
                }
            };
            if !newer {
                continue;
            }
            let name = format!("{}.json", rec.guid);
            match self
                .backend
                .write("affiliations", &name, rec.to_json().to_line().as_bytes())
            {
                Ok(()) => {
                    self.affiliations.insert(rec.guid.clone(), rec);
                    written += 1;
                }
                Err(e) => self.last_error = Some(format!("history write failed: {e}")),
            }
        }
        if written > 0 {
            // A newly named alt may own cards the store-wide owner was
            // stamped on.
            self.repair_owners();
        }
        written
    }

    /// What the addon last saw of a player.
    pub fn affiliation(&self, guid: &str) -> Option<&Affiliation> {
        self.affiliations.get(guid)
    }

    /// Every affiliation, guid-sorted.
    pub fn affiliations(&self) -> Vec<&Affiliation> {
        let mut all: Vec<&Affiliation> = self.affiliations.values().collect();
        all.sort_by(|a, b| a.guid.cmp(&b.guid));
        all
    }

    // ---- creature classifications (the wowdps addon) ----------------------------

    /// Take the addon's NPC classifications in (`creatures::merge`: the
    /// newest sighting per creature id wins, and a row the addon pruned
    /// in-game stays) and, when a row changed, rewrite `creatures.tsv`
    /// whole through `write_atomic`, ids ascending. The number of rows
    /// changed; 0 when nothing did, when the write failed (the rows in
    /// hand stay what is on disk), or when the file on disk is one this
    /// build must not rewrite.
    pub fn merge_creatures(&mut self, recs: Vec<Creature>) -> usize {
        if let Some(why) = &self.creatures_refused {
            self.last_error = Some(format!("{why}; the addon's creatures were not merged"));
            return 0;
        }
        let mut next = self.creatures.clone();
        let changed = creatures::merge(&mut next, recs);
        if changed == 0 {
            return 0;
        }
        match self.backend.write(
            "",
            creatures::FILE,
            creatures::render(next.values()).as_bytes(),
        ) {
            Ok(()) => {
                self.creatures = next;
                changed
            }
            Err(e) => {
                self.last_error = Some(format!("history write failed: {e}"));
                0
            }
        }
    }

    /// `creatures.tsv` as the store holds it, by creature id.
    pub fn creatures(&self) -> &BTreeMap<u32, Creature> {
        &self.creatures
    }

    /// Stamp each player's `guild` from the affiliations — the read-time
    /// join every answered card goes through. A player the addon never saw
    /// stays `None`; one seen without a guild reads `Some("")`.
    fn join_guilds(&self, card: &mut FightCard) {
        if self.affiliations.is_empty() {
            return;
        }
        for p in &mut card.players {
            p.guild = self.affiliations.get(&p.guid).map(|a| a.guild.clone());
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

    /// v39: the fight keeps the series tier — a stored drill answers a
    /// zoom window. v42: in a format this build reads; a file with no
    /// series head, or a later build's, reads as absent (its entry stays in
    /// the map, which retention and the rewrite rule walk).
    pub fn has_series(&self, id: &str) -> bool {
        self.series_format(id)
            .is_some_and(|f| (series_tier::OLDEST..=series_tier::FORMAT).contains(&f))
    }

    /// v39: one player's seconds off the series tier — the file's head, its
    /// index and that player's block alone, never the rest of the raid.
    pub fn player_series(&self, id: &str, guid: &str) -> Option<PlayerSeries> {
        if !self.has_series(id) {
            return None;
        }
        let name = format!("{id}.bin");
        series_tier::read_player(
            |at, len| self.backend.read_range("series", &name, at, len),
            guid,
        )
    }

    /// v39 / v42: a fight that wants a rewrite from its log to be what the
    /// store promises it — one kept whole (a boss kill, a timed key) or
    /// pinned that lost its details (demoted before v42 kept it whole, or
    /// before its pin), or one earning the series tier whose file is
    /// missing (a wipe just pinned, a fight older than the tier) or older
    /// than this build's format (v42's ability targets and 1 s damage
    /// taken). A demoted wipe or over-time key stays demoted.
    pub fn wants_rewrite(&self, id: &str) -> bool {
        let Some(card) = self.card(id) else {
            return false;
        };
        if !self.earns_details(card) {
            return false;
        }
        if !self.has_details(id) {
            // v45: a progression wipe is kept whole, and so rewritten whole.
            return self.kept_whole(card) || card.pinned;
        }
        // Missing, no series head, or older than this build's: rewrite. A
        // NEWER format is a later build's, whose data this one would lose.
        self.earns_series(card)
            && self
                .series_format(id)
                .is_none_or(|f| f < series_tier::FORMAT)
    }

    /// v42: every fight [`Store::wants_rewrite`] names, newest first — what
    /// the history thread rewrites from the logs when its mailbox is idle.
    /// A details stat per card; the series formats are in hand.
    pub fn rewrites(&self) -> Vec<String> {
        self.cards
            .iter()
            .rev()
            .filter(|c| self.wants_rewrite(&c.id))
            .map(|c| c.id.clone())
            .collect()
    }

    /// v45 (R29): every fight that wants only its replay cut
    /// ([`Store::wants_replay_rewrite`], not already a [`Store::rewrites`]),
    /// newest first — the rest of what the thread backfills when idle: the
    /// tier for a store older than it, a progression wipe, a new pin.
    pub fn recuts(&self) -> Vec<String> {
        let protected = self.protected();
        let slots = self.slots_from(&protected);
        self.cards
            .iter()
            .rev()
            .filter(|c| {
                !self.wants_rewrite(&c.id) && self.wants_replay_rewrite(&c.id, &slots, &protected)
            })
            .map(|c| c.id.clone())
            .collect()
    }

    /// v42: the layout a fight's series file is in, as its head names it at
    /// open or this build wrote it — a later build's too; `None` without
    /// one, or for a file with no series head.
    pub fn series_format(&self, id: &str) -> Option<u8> {
        self.series.get(id).copied().flatten()
    }

    /// v42: the fight's series file opens an ability — a format this
    /// build reads that keeps each ability's targets (2 on).
    pub fn has_abilities(&self, id: &str) -> bool {
        self.series_format(id)
            .is_some_and(|f| (2..=series_tier::FORMAT).contains(&f))
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
        self.status_with(self.cfg.characters.is_empty() && self.owner().is_some())
    }

    /// [`Store::status`] with whether an owner resolved already known.
    fn status_with(&self, owned: bool) -> HistoryStatus {
        let (kept_kills, kept_keys, kept_progression, kept_pins) = self.kept_counts();
        HistoryStatus {
            enabled: true,
            fights: self.cards.len() as u32,
            dropped: 0,
            importing: 0,
            owner_inferred: self.cfg.characters.is_empty() && owned,
            error: self.last_error.clone(),
            // The thread fills `addon` in; the store only knows the files.
            addon: None,
            affiliations: self.affiliations.len() as u32,
            affiliations_utc_ms: self.affiliations.values().map(|a| a.seen_utc_ms).max(),
            replays: self.replays.len() as u32,
            replay_bytes: self.replay_total(),
            kept_kills,
            kept_keys,
            kept_progression,
            kept_pins,
        }
    }

    /// Would this scanned segment be stored at all? A pre-filter for the
    /// import path (`keyed` = visit ordinals that were keystone runs, so a
    /// key's bosses are not parsed only to be refused); `wants` is the truth.
    pub fn wants_meta(&self, meta: &SegmentMeta, keyed: &HashSet<u32>) -> bool {
        match meta.kind {
            SegmentKind::Overall => true,
            SegmentKind::Encounter => {
                self.cfg.store_trash
                    || !(meta.visit.is_some_and(|v| keyed.contains(&v))
                        || meta
                            .encounter
                            .is_some_and(|e| e.difficulty == KEYSTONE_DIFFICULTY))
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
            SegmentKind::Encounter => {
                self.cfg.store_trash
                    || !(visit.is_some_and(|v| v.keyed)
                        || seg
                            .encounter
                            .is_some_and(|e| e.difficulty == KEYSTONE_DIFFICULTY))
            }
            SegmentKind::Trash => self.cfg.store_trash && seg.counts(),
        }
    }

    /// Insert-if-absent on the fight id (a record is rewritten only when
    /// its schema is older). Returns the id when something was written.
    pub fn store(&mut self, fight: &ClosedFight, facts: LogFacts) -> Option<String> {
        self.store_impl(fight, facts, false)
    }

    /// Rewrite a fight the store already holds, from a fresh parse — the
    /// path a ruling change takes to old records. Pin and owner ride along;
    /// annotations are separate files and never touched.
    pub fn regrade(&mut self, fight: &ClosedFight, facts: LogFacts) -> Option<String> {
        self.store_impl(fight, facts, true)
    }

    fn store_impl(&mut self, fight: &ClosedFight, facts: LogFacts, force: bool) -> Option<String> {
        if !self.wants(&fight.segment, fight.visit.as_ref()) {
            return None;
        }
        let id = fight_id(
            facts.id,
            fight.segment.start_ms,
            fight.segment.kind == SegmentKind::Overall,
        );
        // An aborted record is provisional: the same fight closing for real
        // (its END arriving after a restart) replaces it.
        if !force
            && let Some(existing) = self.card(&id)
            && existing.schema >= HISTORY_SCHEMA
            && !(existing.aborted && !fight.aborted)
        {
            return None;
        }
        let mut doc = extract(fight, facts, &id);
        doc.card.owner = self.owner_of(&doc.card);
        // A pin is the user's decision: a rewrite (aborted → real, or an
        // older schema) carries it forward.
        doc.card.pinned = self.card(&id).is_some_and(|c| c.pinned);
        // The rows tier always; details for kills and for wipes at least
        // `details_min_wipe_secs` long (retention keeps bests and pins
        // afterwards and caps the rest); loadouts content-addressed.
        let wants_details = self.earns_details(&doc.card);
        let write = |b: &mut B, dir: &str, stem: &str, v: json::Json| -> io::Result<()> {
            b.write(dir, &format!("{stem}.json"), v.to_line().as_bytes())
        };
        let mut result = write(&mut self.backend, "rows", &id, doc.rows.to_json());
        // A rewrite whose new verdict earns no details must not leave the
        // old parse's behind: `has_details` (and so `stored_fight`'s tier)
        // keys off the file alone.
        if result.is_ok() && force && !wants_details {
            let name = format!("{id}.json");
            if self.backend.exists("details", &name) {
                result = self.backend.remove("details", &name);
            }
        }
        if result.is_ok() && wants_details {
            result = write(&mut self.backend, "details", &id, doc.details.to_json());
        }
        // v39: the series tier for the fights that keep it (a kill, a key, a
        // pinned fight — the pin carried above); a rewrite whose verdict no
        // longer earns it drops the old one.
        let series_name = format!("{id}.bin");
        if result.is_ok() && self.earns_series(&doc.card) {
            result = self.backend.write(
                "series",
                &series_name,
                &series_of(&fight.segment, &doc.card).encode(),
            );
            if result.is_ok() {
                self.series.insert(id.clone(), Some(series_tier::FORMAT));
            }
        } else if result.is_ok() && self.series.contains_key(&id) {
            result = self.backend.remove("series", &series_name);
            if result.is_ok() {
                self.series.remove(&id);
            }
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
        // v45: a kill ends its boss's progression; a wipe changes nothing.
        self.refresh_killed();
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
            Ok(()) => {
                // v39: an unpinned wipe's series goes with its pin (v45:
                // unless it is progression, kept whole all the same).
                if self.card(id).is_some_and(|c| !self.earns_series(c)) {
                    self.drop_series(id);
                }
                // v45: and its replay, when no slot holds it any more.
                if !pinned && !self.replay_slots().contains(id) {
                    self.drop_replay(id);
                }
                true
            }
            Err(e) => {
                self.last_error = Some(format!("history write failed: {e}"));
                false
            }
        }
    }

    /// Who "me" is ON THIS CARD: a player of the card who is one of the
    /// account's characters — the configured names first, then the
    /// addon's own-character set, then the store-wide owner when they are
    /// on the roster — else `None`. Never a guid the card does not list:
    /// the store-wide owner is one character, and a night on an alt is
    /// the alt's night (spec §9).
    pub fn owner_of(&self, card: &FightCard) -> Option<String> {
        let wanted: Vec<String> = self
            .cfg
            .characters
            .iter()
            .map(|c| c.trim().to_lowercase())
            .filter(|c| !c.is_empty())
            .collect();
        let configured = card
            .players
            .iter()
            .filter(|p| !p.enemy)
            .find(|p| name_matches(&wanted, &p.name));
        if let Some(p) = configured {
            return Some(p.guid.clone());
        }
        if let Some(p) = card
            .players
            .iter()
            .filter(|p| !p.enemy)
            .find(|p| self.affiliations.get(&p.guid).is_some_and(|a| a.mine))
        {
            return Some(p.guid.clone());
        }
        let (guid, _) = self.owner()?;
        card.players.iter().any(|p| p.guid == guid).then_some(guid)
    }

    /// Re-stamp every card whose owner is missing or not on its roster —
    /// cards written under the store-wide owner before per-card ownership
    /// existed, or before the addon's file named the alt. Returns how many
    /// were rewritten. Runs on open and whenever the addon's set changes.
    pub fn repair_owners(&mut self) -> usize {
        let fixes: Vec<(usize, Option<String>)> = self
            .cards
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.owner
                    .as_deref()
                    .is_none_or(|o| !c.players.iter().any(|p| p.guid == o))
            })
            .filter_map(|(i, c)| {
                let new = self.owner_of(c);
                (c.owner != new).then_some((i, new))
            })
            .collect();
        let mut written = 0;
        for (i, new) in fixes {
            let Some(card) = self.cards.get_mut(i) else {
                continue;
            };
            card.owner = new;
            let doc = card.to_json().to_line();
            let name = format!("{}.json", card.id);
            match self.backend.write("fights", &name, doc.as_bytes()) {
                Ok(()) => written += 1,
                Err(e) => self.last_error = Some(format!("history write failed: {e}")),
            }
        }
        written
    }

    /// v35: every character of the account this store knows — the addon's
    /// own-character set, the store-wide owner and each card's owner (all
    /// three already "me" to the cards), by guid, and the configured names
    /// for a character no card has met yet. What marks an answer's rows
    /// `mine`.
    pub fn mine(&self) -> Mine {
        self.mine_with(self.owner())
    }

    /// [`Store::mine`] over an [`Store::owner`] already resolved — the
    /// intersection behind it walks every card's players.
    fn mine_with(&self, owner: Option<(String, bool)>) -> Mine {
        let guids = self
            .affiliations
            .values()
            .filter(|a| a.mine)
            .map(|a| a.guid.clone())
            .chain(self.cards.iter().filter_map(|c| c.owner.clone()))
            .chain(owner.map(|(g, _)| g));
        Mine::new(guids, &self.cfg.characters)
    }

    /// What the thread publishes after every request: the status and the
    /// account's characters, over ONE resolution of the owner.
    pub fn status_and_mine(&self) -> (HistoryStatus, Mine) {
        let owner = self.owner();
        (self.status_with(owner.is_some()), self.mine_with(owner))
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
            // "Name-Realm" must match whole; a bare "Name" (no realm
            // given) matches the name half.
            return self
                .cards
                .iter()
                .rev()
                .flat_map(|c| c.players.iter())
                .find(|p| name_matches(&wanted, &p.name))
                .map(|p| (p.guid.clone(), false));
        }
        // Spec §9a: the addon marks the account's own characters, and a
        // character the account logged in as IS the logger — whichever alt
        // was on the newest card. It outranks the intersection, which two
        // logs sharing a guildmate can get wrong.
        if let Some(mine) = self
            .cards
            .iter()
            .rev()
            .flat_map(|c| c.players.iter())
            .find(|p| self.affiliations.get(&p.guid).is_some_and(|a| a.mine))
        {
            return Some((mine.guid.clone(), true));
        }
        let mut per_log: HashMap<u64, HashSet<&str>> = HashMap::new();
        for c in &self.cards {
            let set = per_log.entry(c.log).or_default();
            for p in c.players.iter().filter(|p| p.logged && !p.enemy) {
                set.insert(&p.guid);
            }
        }
        let mut logs = per_log.values().filter(|s| !s.is_empty());
        let mut common: HashSet<&str> = logs.next().cloned().unwrap_or_default();
        for s in logs {
            common.retain(|g| s.contains(g));
        }
        // One log alone can't tell the logger from their guildmates; two
        // can only when exactly one name survives the intersection.
        if per_log.len() >= 2 && common.len() == 1 {
            return common.into_iter().next().map(|g| (g.to_string(), true));
        }
        // The inference is sticky: once a main was found and stamped on the
        // cards, one alt's dungeon or a friend's imported log (which empty
        // the intersection) must not un-know them — that would strip every
        // personal best of its retention protection. The newest stamp wins.
        self.cards
            .iter()
            .rev()
            .find_map(|c| c.owner.clone())
            .map(|g| (g, true))
    }

    /// Cards + rows per (kind, encounter or map, difficulty) capped at
    /// `keep_per_encounter`, details at `keep_details_per_encounter`,
    /// oldest first, never touching the protected set: pinned, annotated,
    /// (v42) every boss kill and timed key (`Retention::keeps_whole`), the
    /// fastest kill per group, and the owner's best per (group, spec) for
    /// Damage, Healing and — R17, Tank specs on kills — mitigated_pct. The
    /// caps count the unprotected fights alone (v42).
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
            // v42: the caps count the unprotected alone — the newest N of
            // the fights retention may touch — so a farmed boss's kills,
            // kept whole and uncounted, never squeeze its wipes out.
            let over = unprotected
                .len()
                .saturating_sub(self.cfg.keep_per_encounter);
            evict.extend(unprotected.iter().take(over));
            let with_details: Vec<usize> = unprotected
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
                    .take(over)
                    .filter_map(|i| self.cards.get(*i).map(|c| c.id.clone())),
            );
        }
        for id in demote {
            let _ = self.backend.remove("details", &format!("{id}.json"));
            // v39: a series outlives no details: a window of lists the
            // store no longer keeps would answer nothing.
            self.drop_series(&id);
        }
        let evicted = !evict.is_empty();
        evict.sort_unstable();
        for i in evict.into_iter().rev() {
            if i < self.cards.len() {
                let card = self.cards.remove(i);
                for dir in ["details", "rows", "fights"] {
                    let _ = self.backend.remove(dir, &format!("{}.json", card.id));
                }
                self.drop_series(&card.id);
                self.drop_replay(&card.id);
            }
        }
        // v45: an evicted kill (`history_keep_kills_whole` off) can reopen
        // its boss's progression. The protected set is walked again only
        // when a card went (it can move the owner, the kills): a pass that
        // evicted nothing keeps the one in hand.
        let killed = self.killed.clone();
        self.refresh_killed();
        let protected = if !evicted && self.killed == killed {
            protected
        } else {
            self.protected()
        };
        // v39: and none outlives its reason — an unpinned wipe's goes.
        let unwanted: Vec<String> = self
            .series
            .keys()
            .filter(|id| self.card(id).is_none_or(|c| !self.earns_series(c)))
            .cloned()
            .collect();
        for id in unwanted {
            self.drop_series(&id);
        }
        // v45 (R29): a replay outside the slots is demoted with the details
        // caps, and past the size cap the oldest unprotected go next — never
        // a protected one (kills, timed keys, progression, pins …).
        let slots = self.slots_from(&protected);
        let unslotted: Vec<String> = self
            .replays
            .keys()
            .filter(|id| !slots.contains(*id))
            .cloned()
            .collect();
        for id in unslotted {
            self.drop_replay(&id);
        }
        self.trim_replays(&protected);
    }

    /// v39: remove a fight's series tier, if it has one.
    fn drop_series(&mut self, id: &str) {
        if self.series.remove(id).is_some() {
            let _ = self.backend.remove("series", &format!("{id}.bin"));
        }
    }

    fn protected(&self) -> HashSet<String> {
        let mut out: HashSet<String> = HashSet::new();
        let owner = self.owner().map(|(g, _)| g);
        let mut fastest: HashMap<GroupKey, (i64, &str)> = HashMap::new();
        // (group, spec id, 0 = damage / 1 = healing / 2 = mitigated_pct) →
        // the owner's best value. Zeros never enter (see below).
        let mut best: HashMap<(GroupKey, u32, u8), (f64, &str)> = HashMap::new();
        for c in &self.cards {
            // v42: a boss kill and a timed key are kept whole, all season.
            if c.pinned
                || self.kept_whole(c)
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
            // The owner's personal bests. A best is only a best when it is
            // a real number on a real fight: an aborted record never
            // qualifies, and a measure of 0 protects nothing (before the
            // floor, "best hps = 0.0" pinned an arbitrary card on every
            // pure-DPS spec, and every un-regraded card would now do the
            // same for mitigated_pct).
            if let Some(owner) = &owner
                && !c.aborted
                && let Some(p) = c.players.iter().find(|p| &p.guid == owner)
            {
                let spec = p.spec.map_or(0, |s| s.id());
                // 0 damage, 1 healing, 2 (R17) mitigated_pct — the tank
                // measure, kills only, and only for a Tank spec: a DPS's
                // incidental mitigation is not an achievement to protect.
                let tank_pct = (p.role() == Some(Role::Tank) && c.success == Some(true))
                    .then(|| p.mitigated_pct());
                for (view, per_sec) in [(0u8, Some(p.dps)), (1u8, Some(p.hps)), (2u8, tank_pct)] {
                    let Some(per_sec) = per_sec.filter(|v| *v > 0.0) else {
                        continue;
                    };
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
                after_id,
                role,
            } => {
                // v22: `role` is the SUBJECT's role — `guid` when one was
                // given, else the owner. With no subject at all (owner
                // uninferred and no guid) the filter is a no-op, resolved
                // inside `fights`.
                let (cards, total) = self.fights(
                    *encounter,
                    *difficulty,
                    guid.as_deref(),
                    *since_utc_ms,
                    *kind,
                    *sort,
                    *limit,
                    after_id.as_deref(),
                    *role,
                );
                HistoryAnswer::Fights { cards, total }
            }
            HistoryQuery::Progression {
                encounter,
                difficulty,
                local_cutover_hour,
            } => self.progression(*encounter, *difficulty, *local_cutover_hour),
            HistoryQuery::Trend {
                guid,
                spec,
                encounter,
                difficulty,
                measure,
                bucket,
                since_utc_ms,
                limit,
                local_cutover_hour,
            } => HistoryAnswer::Trend(self.trend(
                guid,
                *spec,
                *encounter,
                *difficulty,
                *measure,
                *bucket,
                *since_utc_ms,
                *limit,
                *local_cutover_hour,
            )),
            HistoryQuery::RoleNight {
                encounter,
                difficulty,
                night,
                local_cutover_hour,
            } => self.role_night(*encounter, *difficulty, *night, *local_cutover_hour),
        }
    }

    /// `Fastest` considers kills only (best kill = `Fastest`, limit 1);
    /// `OwnerPerSec` ranks by the owner's damage per second and needs an
    /// owner; `limit` 0 means 50.
    ///
    /// v22 `role`: only fights the SUBJECT played that role in, by their
    /// spec on that card — the subject is `guid` when one was given, else
    /// the owner. With neither (the owner is uninferred and no `guid` was
    /// asked for) there is nobody whose role to read, so the filter is a
    /// no-op and every fight still answers.
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
        after_id: Option<&str>,
        role: Option<Role>,
    ) -> (Vec<FightCard>, u32) {
        let owner = self.owner().map(|(g, _)| g);
        let subject: Option<&str> = guid.or(owner.as_deref());
        let mut hits: Vec<&FightCard> = self
            .cards
            .iter()
            .filter(|c| match (role, subject) {
                (Some(role), Some(subject)) => c
                    .players
                    .iter()
                    .any(|p| p.guid == subject && p.role() == Some(role)),
                // No role asked, or no subject to read one off: no-op.
                _ => true,
            })
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
        let total = hits.len() as u32;
        // A card is small but not free (its whole player list rides along),
        // and `wire::frame` only `debug_assert!`s on `MAX_FRAME`: a release
        // daemon asked for every card in a season's lake would emit a frame
        // the reader rejects, which reads to the client as a reconnect loop.
        // Cap the page here so no client can ask for an unsendable answer;
        // `total` is unclamped, so a pager still knows what it has not seen.
        let limit = if limit == 0 { 50 } else { limit as usize }.min(FIGHTS_CAP);
        // Paging: resume right after the id the last page ended on. An id
        // the sorted set does not hold (evicted, or a stale cursor) starts
        // from the top rather than answering nothing.
        let skip = after_id
            .and_then(|id| hits.iter().position(|c| c.id == id))
            .map_or(0, |i| i + 1);
        // "Me" is resolved now, not when the card was written: a card from
        // before `history_characters` was set still names the owner.
        let cards = hits
            .into_iter()
            .skip(skip)
            .take(limit)
            .cloned()
            .map(|mut c| {
                if c.owner.is_none() {
                    c.owner = self.owner_of(&c);
                }
                // Likewise the guilds: what the addon knows NOW, on a card
                // written before its file landed.
                self.join_guilds(&mut c);
                c
            })
            .collect();
        (cards, total)
    }

    fn progression(&self, encounter: u32, difficulty: u32, cutover: Option<u8>) -> HistoryAnswer {
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
        let first_kill = kills.iter().min_by_key(|c| c.start_utc_ms).map(|c| {
            let mut c = (*c).clone();
            if c.owner.is_none() {
                c.owner = self.owner_of(&c);
            }
            Box::new(c)
        });
        let mut nights: BTreeMap<i64, Night> = BTreeMap::new();
        for c in &pulls {
            let day = bucket_start(c.start_utc_ms, c.tz_min, cutover, false);
            let n = nights.entry(day).or_insert(Night {
                day_utc_ms: day,
                pulls: 0,
                kill: false,
                kills: 0,
                best_pct: None,
                tz_min: c.tz_min,
            });
            n.pulls += 1;
            n.kill |= c.success == Some(true);
            n.kills += u32::from(c.success == Some(true));
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

    /// v26 (step 5): one night of one boss folded per friendly player —
    /// the night's non-aborted pulls at `encounter` / `difficulty` (the
    /// `progression` match) whose `bucket_start` day is `night`, the
    /// `Night` built exactly as `progression` builds that bucket. Per
    /// player: `measure` / `best` are the mean / max over pulls of the
    /// role measure — `effective_dps` for dps, `hps` for healers,
    /// `mitigated_pct` for tanks, 0 with no role — `taken` and
    /// `externals_given` sums, `dtps` / `am_uptime_pct` / `overheal_pct`
    /// means, `absorb_efficiency` a RATIO OF SUMS over the pulls whose
    /// waste is known (`None` when none is, or the sums are 0). Every mean
    /// is `Σ / n as f64`, the pulls walked in start order, the arithmetic
    /// the lake's SQL twin mirrors. `name` is the last seen, `spec` the
    /// most-played (specless pulls ignored; a tie → the smallest id) and
    /// `role` that spec's — picked FIRST, and then `pulls` and every fold
    /// count ONLY the pulls the player played in that role, so a spec-swap
    /// night (a tank pull, then dps) has one denominator per column; a
    /// player whose every pull is specless gets role `None`, measure 0 and
    /// all their pulls.
    /// Rows: tank, healer, dps, no-role last, then `measure` desc, then
    /// guid. An empty night is the `Night` with 0 pulls and no rows.
    fn role_night(
        &self,
        encounter: u32,
        difficulty: u32,
        night: i64,
        cutover: Option<u8>,
    ) -> HistoryAnswer {
        let mut pulls: Vec<&FightCard> = self
            .cards
            .iter()
            .filter(|c| {
                c.encounter
                    .is_some_and(|e| e.id == encounter && e.difficulty == difficulty)
                    && !c.aborted
                    && bucket_start(c.start_utc_ms, c.tz_min, cutover, false) == night
            })
            .collect();
        pulls.sort_by_key(|c| c.start_utc_ms);
        let mut summary = Night {
            day_utc_ms: night,
            pulls: 0,
            kill: false,
            kills: 0,
            best_pct: None,
            tz_min: pulls.first().and_then(|c| c.tz_min),
        };
        // Pass 1: the roster — every friendly (card, player) pair per guid
        // in start order, and the spec census the mode is picked from.
        struct Seen<'a> {
            name: String,
            specs: BTreeMap<u32, u32>,
            pulls: Vec<(&'a FightCard, &'a CardPlayer)>,
        }
        let mut seen: HashMap<&str, Seen<'_>> = HashMap::new();
        for c in &pulls {
            summary.pulls += 1;
            summary.kill |= c.success == Some(true);
            summary.kills += u32::from(c.success == Some(true));
            // R16: the night's lowest.
            summary.best_pct = match (summary.best_pct, c.best_pct) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            for p in c.players.iter().filter(|p| !p.enemy) {
                let s = seen.entry(p.guid.as_str()).or_insert_with(|| Seen {
                    name: String::new(),
                    specs: BTreeMap::new(),
                    pulls: Vec::new(),
                });
                s.name.clone_from(&p.name);
                if let Some(spec) = p.spec {
                    *s.specs.entry(spec.id()).or_insert(0) += 1;
                }
                s.pulls.push((c, p));
            }
        }
        // Pass 2: the mode spec FIRST, then one fold over only the pulls
        // played in its role — one denominator for every column.
        let mut rows: Vec<RoleNightRow> = seen
            .into_iter()
            .map(|(guid, s)| {
                // The mode; the BTreeMap walks ids ascending and a strict
                // `>` keeps the first, so a tie lands on the smallest id.
                let spec = s
                    .specs
                    .iter()
                    .fold(None, |best: Option<(u32, u32)>, (&id, &k)| match best {
                        Some((_, bk)) if bk >= k => best,
                        _ => Some((id, k)),
                    })
                    .map(|(id, _)| id);
                let role = spec.and_then(Spec::from_id).map(Spec::role);
                let mut n = 0u32;
                let mut measure_sum = 0.0;
                let mut best = 0.0;
                let mut taken = 0u64;
                let mut dtps_sum = 0.0;
                let mut am_sum = 0.0;
                let mut overheal_sum = 0.0;
                let mut absorbed_known = 0u64;
                let mut wasted = 0u64;
                let mut known = false;
                let mut externals_given = 0u32;
                for (c, p) in s.pulls.iter().filter(|(_, p)| p.role() == role) {
                    let m = match role {
                        Some(Role::Dps) => p.effective_dps(c.rate_ms()),
                        Some(Role::Healer) => p.hps,
                        Some(Role::Tank) => p.mitigated_pct(),
                        None => 0.0,
                    };
                    n += 1;
                    measure_sum += m;
                    if n == 1 || m > best {
                        best = m;
                    }
                    taken += p.taken;
                    dtps_sum += p.dtps;
                    am_sum += p.am_uptime_pct(c.duration_ms);
                    let heal_total = p.healing + p.overheal;
                    overheal_sum += if heal_total > 0 {
                        p.overheal as f64 * 100.0 / heal_total as f64
                    } else {
                        0.0
                    };
                    if let Some(w) = p.absorb_wasted {
                        known = true;
                        absorbed_known += p.absorbed;
                        wasted += w;
                    }
                    externals_given += p.externals_given;
                }
                let nf = f64::from(n);
                let total = absorbed_known + wasted;
                RoleNightRow {
                    guid: guid.to_string(),
                    name: s.name,
                    spec: spec.and_then(|id| u16::try_from(id).ok()),
                    role,
                    pulls: n,
                    measure: measure_sum / nf,
                    best,
                    taken,
                    dtps: dtps_sum / nf,
                    am_uptime_pct: am_sum / nf,
                    overheal_pct: overheal_sum / nf,
                    absorb_efficiency: (known && total > 0)
                        .then(|| absorbed_known as f64 / total as f64),
                    externals_given,
                }
            })
            .collect();
        let rank = |r: Option<Role>| match r {
            Some(Role::Tank) => 0,
            Some(Role::Healer) => 1,
            Some(Role::Dps) => 2,
            None => 3,
        };
        rows.sort_by(|a, b| {
            rank(a.role)
                .cmp(&rank(b.role))
                .then(b.measure.total_cmp(&a.measure))
                .then_with(|| a.guid.cmp(&b.guid))
        });
        HistoryAnswer::RoleNight {
            night: summary,
            rows,
        }
    }

    /// One point per fight (newest first), or per UTC day / week with
    /// `per_sec` averaged and `amount` / `duration_ms` summed.
    ///
    /// v22: `measure` picks what a point carries — dps / hps / dtps, or the
    /// derived `mitigated_pct`. A `Day` / `Week` bucket folds `per_sec` as a
    /// running MEAN of the per-fight values, never `amount / duration_ms`:
    /// for MitigatedPct that is a mean of pcts, exactly as Dps-by-day is
    /// already a mean of rates (CONTRACT v22).
    #[allow(clippy::too_many_arguments)]
    fn trend(
        &self,
        guid: &str,
        spec: Option<u32>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        measure: TrendMeasure,
        bucket: TrendBucket,
        since_utc_ms: Option<i64>,
        limit: u32,
        cutover: Option<u8>,
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
                // v22: `amount` is the measure's numerator and `per_sec`
                // its value — a rate for the three rate measures, the
                // percentage for MitigatedPct (derived on the card).
                let (amount, per_sec) = match measure {
                    TrendMeasure::Dps => (p.damage, p.dps),
                    TrendMeasure::Hps => (p.healing, p.hps),
                    TrendMeasure::Dtps => (p.taken, p.dtps),
                    TrendMeasure::MitigatedPct => (p.mitigated, p.mitigated_pct()),
                    // v23 (R19): the numerator is `effective` and the
                    // rate is it over the card's own duration — `dps` bit
                    // for bit on a fight without support.
                    TrendMeasure::EffectiveDps => (p.effective(), p.effective_dps(c.rate_ms())),
                    // v25 (R18, step 4b): the numerator is the AM union in ms
                    // and the value its percentage of the card's duration.
                    TrendMeasure::AmUptime => (p.am_uptime_ms, p.am_uptime_pct(c.duration_ms)),
                    // v26 (R20, step 5): the numerator is the absorbed total
                    // and the value the efficiency as a percentage; a card
                    // whose waste is unknown (`None`) contributes NO point,
                    // so a bucket's running mean is over the known cards
                    // only (`n` counts them).
                    TrendMeasure::AbsorbEfficiency => (p.absorbed, p.absorb_efficiency()? * 100.0),
                };
                Some(TrendPoint {
                    bucket_utc_ms: match bucket {
                        TrendBucket::None => c.start_utc_ms,
                        TrendBucket::Day => bucket_start(c.start_utc_ms, c.tz_min, cutover, false),
                        TrendBucket::Week => bucket_start(c.start_utc_ms, c.tz_min, cutover, true),
                    },
                    fight_id: c.id.clone(),
                    spec: p_spec,
                    amount,
                    per_sec,
                    duration_ms: c.duration_ms,
                    n: 1,
                    tz_min: c.tz_min,
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
    /// Damage and Healing (absent when demoted, or never written: short
    /// wipes and aborted fights have none — Healing then serves the coarse
    /// `heal10` alone), the death recap from the rows tier for Deaths, the
    /// mitigation lists + the coarse taken series for Taken (see
    /// `drill_of`), and — v25 — the player's `uptime`, both halves (see
    /// `uptime_of`).
    pub fn stored_fight(
        &self,
        id: &str,
        view: View,
        drill: Option<&str>,
        death: Option<u32>,
    ) -> Option<StoredFight> {
        self.stored_fight_as(&self.mine(), id, view, drill, death)
    }

    /// [`Store::stored_fight`] with the account's characters resolved
    /// already — the thread's last published [`Mine`], so an answer does
    /// not walk every card again for them.
    pub fn stored_fight_as(
        &self,
        mine: &Mine,
        id: &str,
        view: View,
        drill: Option<&str>,
        death: Option<u32>,
    ) -> Option<StoredFight> {
        self.stored_fight_in(mine, id, &Ask::of(view, drill, death))
    }

    /// v39: [`Store::stored_fight`] with a zoom window: a Damage or Healing
    /// drill of a fight that keeps the series tier answers its lists for
    /// `range` (snapped out to whole seconds) as the live drill does.
    pub fn stored_fight_ranged(
        &self,
        id: &str,
        view: View,
        drill: Option<&str>,
        death: Option<u32>,
        range: Option<(u32, u32)>,
    ) -> Option<StoredFight> {
        let ask = Ask {
            range,
            ..Ask::of(view, drill, death)
        };
        self.stored_fight_in(&self.mine(), id, &ask)
    }

    /// v42: a stored fight answered for whatever `ask` asks, with the
    /// account's characters resolved — every reader's one path. What the
    /// series tier adds (a fight that keeps it, with its details): a zoom
    /// window's lists (v39), the drill's stacked series, an opened ability
    /// and the 1 s damage taken, as the live drill answers them; a pair's
    /// comparison needs the details tier alone (`answer`).
    pub fn stored_fight_in(&self, mine: &Mine, id: &str, ask: &Ask) -> Option<StoredFight> {
        let mut card = self.card(id)?.clone();
        self.join_guilds(&mut card);
        // The card alone is an answer: rows and details tiers can be gone
        // (retention demotes details, and rows only ever go with the card,
        // but a torn file reads as absent) — the reader sees `tier` and
        // says what it could not serve.
        let details = self.details(id);
        let Some(rows_doc) = self.rows(id) else {
            return Some(StoredFight {
                card,
                rows: Vec::new(),
                breakdown: None,
                tier: 1,
                has_recap: false,
                loadout: None,
                support: None,
                uptime: Vec::new(),
                shields: Vec::new(),
                raid: None,
                series: false,
                abilities: false,
                pair: None,
                energize: Vec::new(),
                power: Vec::new(),
            });
        };
        let tier = if details.is_some() { 3 } else { 2 };
        let loadout = ask.drill.as_deref().and_then(|guid| {
            let hash = card.players.iter().find(|p| p.guid == guid)?.loadout?;
            self.loadout(hash).map(|l| l.loadout)
        });
        // The series tier answers only beside its details: a window of
        // lists the store no longer keeps would answer nothing.
        let read = |guid: &str| self.player_series(id, guid);
        let series = (self.has_series(id) && details.is_some()).then_some(SeriesTier {
            read: &read,
            abilities: self.has_abilities(id),
        });
        Some(answer(
            mine,
            card,
            &rows_doc,
            details.as_ref(),
            tier,
            series,
            loadout,
            ask,
        ))
    }

    /// A `StoredFight` for a fight that is NOT stored — a key's member boss
    /// parsed from the log on demand — in the same shapes the stored path
    /// answers, nothing written. The details tier is in hand (it was just
    /// parsed), so a drill always serves.
    pub fn derived_fight(
        &self,
        fight: &ClosedFight,
        facts: LogFacts,
        view: View,
        drill: Option<&str>,
        death: Option<u32>,
    ) -> StoredFight {
        self.derived_fight_as(&self.mine(), fight, facts, view, drill, death)
    }

    /// [`Store::derived_fight`] with the account's characters resolved.
    pub fn derived_fight_as(
        &self,
        mine: &Mine,
        fight: &ClosedFight,
        facts: LogFacts,
        view: View,
        drill: Option<&str>,
        death: Option<u32>,
    ) -> StoredFight {
        self.derived_fight_in(mine, fight, facts, &Ask::of(view, drill, death))
    }

    /// v42: [`Store::derived_fight_as`] for whatever `ask` asks — through
    /// the same `answer` over the same extract `stored_fight_in` reads back
    /// from its files, so the two paths agree byte for byte. Every tier is
    /// in hand (it was just parsed): a window, an opened ability and a pair
    /// always answer, and a player the series holds no block for had none —
    /// empty lists, as the stored path reads them (`series::read_player`).
    pub fn derived_fight_in(
        &self,
        mine: &Mine,
        fight: &ClosedFight,
        facts: LogFacts,
        ask: &Ask,
    ) -> StoredFight {
        let id = fight_id(
            facts.id,
            fight.segment.start_ms,
            fight.segment.kind == SegmentKind::Overall,
        );
        let mut docs = extract(fight, facts, &id);
        docs.card.owner = self.owner_of(&docs.card);
        let loadout = ask.drill.as_deref().and_then(|guid| {
            let hash = docs.card.players.iter().find(|p| p.guid == guid)?.loadout?;
            docs.loadouts
                .iter()
                .find(|l| l.hash == hash)
                .map(|l| l.loadout.clone())
        });
        // The series only a drill of it uses, built here: `extract` builds
        // none.
        let series = series_of(&fight.segment, &docs.card);
        let read = |guid: &str| {
            Some(
                series
                    .players
                    .iter()
                    .find(|p| p.guid == guid)
                    .cloned()
                    .unwrap_or_else(|| PlayerSeries {
                        guid: guid.to_string(),
                        ..PlayerSeries::default()
                    }),
            )
        };
        answer(
            mine,
            docs.card.clone(),
            &docs.rows,
            Some(&docs.details),
            3,
            Some(SeriesTier {
                read: &read,
                abilities: true,
            }),
            loadout,
            ask,
        )
    }

    pub fn corrupt(&self) -> u32 {
        self.corrupt
    }
}

const DAY_MS: i64 = 86_400_000;

/// The bucket a fight falls into, as its start in UTC ms: the UTC calendar
/// day, or — with a cutover hour — the LOCAL day (the log's timezone) that
/// begins at that hour, so an evening running past local midnight stays one
/// night. `week` folds seven such days, weeks starting Monday.
fn bucket_start(
    start_utc_ms: i64,
    tz_min: Option<i16>,
    cutover_hour: Option<u8>,
    week: bool,
) -> i64 {
    let tz = i64::from(tz_min.unwrap_or(0)) * 60_000;
    let (local, shift) = match cutover_hour {
        Some(h) => (start_utc_ms + tz, i64::from(h) * 3_600_000),
        None => (start_utc_ms, 0),
    };
    let shifted = local - shift;
    let day = if week {
        // Epoch day 0 was a Thursday; shift so weeks start Monday.
        (shifted - 4 * DAY_MS).div_euclid(7 * DAY_MS) * 7 * DAY_MS + 4 * DAY_MS
    } else {
        shifted.div_euclid(DAY_MS) * DAY_MS
    };
    let local_start = day + shift;
    match cutover_hour {
        Some(_) => local_start - tz,
        None => local_start,
    }
}

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
    // Never finished: an END-less boss or match, or a keystone whose
    // CHALLENGE_MODE_END never came (left, or the log ended inside it).
    let aborted = fight.aborted
        || (matches!(kind, FightKind::Encounter | FightKind::Arena) && seg.success.is_none())
        || (kind == FightKind::Key && fight.visit.as_ref().is_some_and(|v| v.completed.is_none()));

    let mut views: [Vec<wowdps_core::model::Row>; View::COUNT] = Default::default();
    for (slot, (view, _)) in views
        .iter_mut()
        .zip(wowdps_proto::history::VIEW_KEYS.iter())
    {
        *slot = seg.rows(*view);
    }
    let by_view = |v: View| views.get(v.index()).map_or(&[][..], Vec::as_slice);

    // Players: the union of everyone with a meter row, denormalized. R17
    // (step 2b): the Taken view joins the union, so a player who did
    // nothing but get swung at — a dodged-only row, count > 0 and amount 0
    // — is on the card too. That grows the friendly set `content_id`
    // hashes: a card's `id` never moves (it is the log + start), but its
    // `content` may differ from a PR #16 write of the same fight.
    let mut order: Vec<String> = Vec::new();
    let mut players: HashMap<String, CardPlayer> = HashMap::new();
    for view in [View::Damage, View::Healing, View::Deaths, View::Taken] {
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
                // R2 amendment (step 3b): the row's `extra` is the
                // overhealing — the half of the healing split the row
                // itself carries; `absorbed` comes from the meter below.
                View::Healing => {
                    p.healing = r.amount;
                    p.hps = r.per_sec;
                    p.overheal = r.extra;
                    // v44 (R2): and the row carries the third half too — the
                    // part a heal-absorb ate.
                    p.heal_absorbed = r.heal_absorbed;
                }
                // R17: the same path as `dps` — the row's own rate over the
                // R7 duration, so a stored dtps equals the live snapshot's.
                View::Taken => {
                    p.taken = r.amount;
                    p.dtps = r.per_sec;
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
    // R19 (step 3b): every player the support ledger answers for joins the
    // roster — a supporter the log only ever trails with (no hit, no heal,
    // never swung at) has no row on any view, yet their `given` is what
    // nets the buffed players' `received`: without them Σ effective over
    // the card would be short of Σ damage. Like the Taken join above this
    // can grow the friendly set `content_id` hashes; the id never moves.
    // Their name is whatever the meter knows — the guid itself when the
    // log never named them.
    for r in seg.supporters() {
        players.entry(r.key.clone()).or_insert_with(|| {
            order.push(r.key.clone());
            CardPlayer {
                guid: r.key.clone(),
                name: r.label.clone(),
                class: r.class,
                spec: r.spec,
                enemy: false,
                ..CardPlayer::default()
            }
        });
    }
    let mut loadouts: Vec<StoredLoadout> = Vec::new();
    // R20: each friendly player's ledger rows, folded ONCE — the card's
    // `shields_unknown` and the rows tier's `shields[]` both read them.
    let mut shield_rows: HashMap<String, Vec<ShieldRow>> = HashMap::new();
    // R21 (step 6): the stack ledger per friendly player — the debuffs
    // seen and the raw per-level cells (never the derived level 0).
    let mut stack_blocks: HashMap<String, PlayerStacks> = HashMap::new();
    for guid in &order {
        let Some(p) = players.get_mut(guid) else {
            continue;
        };
        if let Some(l) = seg.loadout(guid) {
            let hash = loadout_hash(l);
            p.loadout = Some(hash);
            p.logged = true;
            if !loadouts.iter().any(|s| s.hash == hash) {
                loadouts.push(StoredLoadout::new(l.clone()));
            }
        }
        // R17: the card's two record-side measures; `mitigated_pct` is
        // derived from them and `taken` on read and never stored in memory.
        if let Some(m) = seg.mitigation(guid) {
            p.mitigated = m.mitigated();
            p.prevented = m.prevented();
            p.reduced = m.reduced;
        }
        // Step 3b: the healing split's absorb half (the absorber-credited
        // R3 total, ≤ the Healing row), the DAMAGE halves of the support
        // ledger (healing shares stay on the rows tier), and the R2
        // amendment's healing received with its self-cast subset.
        // `effective_dps` is derived from these on write, never held.
        p.absorbed = seg.absorbed_healing(guid);
        if let Some(s) = seg.support(guid) {
            p.support_given = s.given_damage;
            p.support_received = s.received_damage;
        }
        if let Some(h) = seg.healed(guid) {
            p.healed_received = h.received;
            p.self_healed = h.self_healed;
        }
        // R18 (step 4b): the AM union (clamped at the R7 clock by the
        // engine, so never over the duration; a Trash card stores the
        // clamped value too) and the externals scalars, raw — the pct is
        // derived on write from the card's duration and never held.
        // FRIENDLY players only: an enemy (an arena's other team) is never
        // graded, and `uptime[]` / `coarse[]` below are friendly-only, so
        // an enemy healer's card externals would have no rollup cells to
        // balance them — Σ uptime.total_ms per caster = the caster's card
        // externals_given_ms must hold on an arena lake too. Enemies store
        // five zeros.
        if !p.enemy {
            p.am_uptime_ms = u64::try_from(seg.am_uptime_ms(guid)).unwrap_or(0);
            let (given, given_ms) = seg.externals_given(guid);
            let (received, received_ms) = seg.externals_received(guid);
            p.externals_given = given;
            p.externals_given_ms = u64::try_from(given_ms).unwrap_or(0);
            p.externals_received = received;
            p.externals_received_ms = u64::try_from(received_ms).unwrap_or(0);
            // R20 (step 5): the shield ledger's two card scalars — the
            // waste `None` when no closed shield had a known one (never 0,
            // which would claim a perfect efficiency) and the unknown
            // count. Enemies keep None / 0 like the R18 scalars above.
            p.absorb_wasted = seg.absorb_wasted(guid);
            let rows = seg.shields(guid);
            p.shields_unknown = rows.iter().map(|r| r.unknown).sum();
            if !rows.is_empty() {
                shield_rows.insert(guid.clone(), rows);
            }
            let debuffs = seg.stacking_debuffs(guid);
            let cells = seg.stack_cells(guid);
            let dropped = seg.stacks_dropped(guid);
            let base = seg.stack_base(guid);
            if !debuffs.is_empty() || !cells.is_empty() || dropped > 0 || !base.is_empty() {
                stack_blocks.insert(
                    guid.clone(),
                    PlayerStacks {
                        guid: guid.clone(),
                        dropped,
                        debuffs,
                        cells,
                        base,
                    },
                );
            }
        }
    }
    let players: Vec<CardPlayer> = order.iter().filter_map(|g| players.remove(g)).collect();

    // R20 (step 5): the rows tier's shield ledger — one block per friendly
    // player with any row (owner-folded per spell by the engine, an open
    // shield folded with its consumed at read time).
    let shields: Vec<PlayerShields> = players
        .iter()
        .filter(|p| !p.enemy)
        .filter_map(|p| {
            let rows = shield_rows.remove(&p.guid)?;
            Some(PlayerShields {
                guid: p.guid.clone(),
                rows,
            })
        })
        .collect();
    // R21 (step 6): the rows tier's stack ledger, in the players' order.
    let stacks: Vec<PlayerStacks> = players
        .iter()
        .filter(|p| !p.enemy)
        .filter_map(|p| stack_blocks.remove(&p.guid))
        .collect();

    // R18 (step 4b): the uptime rollup keyed by TARGET — one block per
    // friendly player with any cell, uncapped, each cell's `src` the
    // caster. A supporter's per-target uptime is read off OTHER blocks by
    // `src`, so nothing is stored twice. The roster is the card's roster:
    // a player who did nothing on any view but wore an aura has no block.
    let uptime: Vec<PlayerUptime> = players
        .iter()
        .filter(|p| !p.enemy)
        .filter_map(|p| {
            let cells = seg.uptime(&p.guid);
            (!cells.is_empty()).then(|| PlayerUptime {
                guid: p.guid.clone(),
                cells,
            })
        })
        .collect();
    // R18 (step 4b): the coarse series — taken and healing at a fixed
    // 10 s — and the ONE merged mark list (item marks + role spans, the
    // list every drill's marks are) for every friendly player with a
    // nonzero bucket or any mark. `COARSE_FACTOR` × the engine's 1 s grid
    // = `COARSE_BUCKET_MS`; `bucket_ms` is not stored.
    let coarse: Vec<PlayerCoarse> = players
        .iter()
        .filter(|p| !p.enemy)
        .filter_map(|p| {
            let taken10 = seg.taken_timeline(&p.guid).coarsen(COARSE_FACTOR).buckets;
            let heal10 = seg.heal_timeline(&p.guid).coarsen(COARSE_FACTOR).buckets;
            let marks = seg.timeline(&p.guid).marks;
            let any = taken10.iter().any(|b| *b != 0)
                || heal10.iter().any(|b| *b != 0)
                || !marks.is_empty();
            any.then(|| PlayerCoarse {
                guid: p.guid.clone(),
                taken10,
                heal10,
                marks,
            })
        })
        .collect();

    // R19 (step 3b): one block per friendly player with any support —
    // given or received, damage or healing — with their target table
    // (`support_targets`, empty for a player who only received). A ledger
    // of all zeros (a fully-overhealed heal share) writes nothing: the
    // block exists to carry numbers.
    let support: Vec<PlayerSupport> = players
        .iter()
        .filter(|p| !p.enemy)
        .filter_map(|p| {
            let s = seg.support(&p.guid)?;
            if s == wowdps_core::model::Support::default() {
                return None;
            }
            Some(PlayerSupport {
                guid: p.guid.clone(),
                given_damage: s.given_damage,
                given_healing: s.given_healing,
                received_damage: s.received_damage,
                received_healing: s.received_healing,
                targets: seg.support_targets(&p.guid),
            })
        })
        .collect();

    // R17 (step 2b): every friendly player who was swung at — one with a
    // Taken row (a miss alone earns one) or a record — carries their record
    // and both Taken drills on the rows tier, on EVERY fight.
    let mitigation: Vec<PlayerMitigation> = players
        .iter()
        .filter(|p| !p.enemy)
        .filter_map(|p| {
            let has_row = by_view(View::Taken).iter().any(|r| r.key == p.guid);
            let record = seg.mitigation(&p.guid);
            if !has_row && record.is_none() {
                return None;
            }
            let (spells, sources) = seg.breakdown(&p.guid, View::Taken);
            let (taken_spells, other) = cap_taken(spells);
            let (taken_sources, other_sources) = cap_taken(sources);
            Some(PlayerMitigation {
                guid: p.guid.clone(),
                record: record.unwrap_or_default(),
                taken_spells,
                other,
                taken_sources,
                other_sources,
            })
        })
        .collect();

    // v28 (R9): one stored window PER DEATH, oldest first. Storing only the
    // last one hid every earlier death of anyone who died twice, and a
    // stored fight is exactly where that matters — nobody re-watches a raid
    // night live.
    let recaps: Vec<Recap> = players
        .iter()
        .filter(|p| p.deaths > 0)
        .flat_map(|p| {
            let dropped = seg.deaths_dropped(&p.guid);
            seg.death_windows(&p.guid)
                .into_iter()
                .map(|(index, ts)| {
                    let (events, attackers) = seg.breakdown_at(&p.guid, View::Deaths, Some(index));
                    Recap {
                        guid: p.guid.clone(),
                        events,
                        attackers,
                        index,
                        at_ms: (ts - seg.start_ms).max(0),
                        dropped,
                    }
                })
                .collect::<Vec<_>>()
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
                // R26 (v36): how each list nests, so a stored pull's abilities
                // group and split like the live one's.
                damage_tree: seg.spell_tree(&p.guid, View::Damage),
                heal_tree: seg.spell_tree(&p.guid, View::Healing),
                // v42: the count views' drills, so a stored pull drills and
                // compares them too. A view the player has nothing on is
                // left out.
                counts: COUNT_VIEWS
                    .iter()
                    .filter_map(|&view| {
                        let (spells, targets) = seg.breakdown(&p.guid, view);
                        (!spells.is_empty() || !targets.is_empty()).then_some(CountDetail {
                            view,
                            spells,
                            targets,
                        })
                    })
                    .collect(),
                // v43 (R27): what energized them, per power type.
                energize: seg.energize(&p.guid),
                // v44 (R28): their pools second by second, per power type.
                power: seg.power(&p.guid),
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
        // v40: the clock the rows' rates below were divided by.
        combat_ms: Some(seg.combat_ms(now)),
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
        bosses: fight
            .members
            .iter()
            .map(|m| KeyBoss {
                start_utc_ms: m.start_utc_ms - i64::from(tz.unwrap_or(0)) * 60_000,
                ..m.clone()
            })
            .collect(),
        players,
    };
    FightDocs {
        card,
        rows: FightRows {
            schema: HISTORY_SCHEMA,
            id: id.to_string(),
            views,
            recaps,
            mitigation,
            support,
            uptime,
            coarse,
            shields,
            stacks,
        },
        details: FightDetails {
            schema: HISTORY_SCHEMA,
            id: id.to_string(),
            players: details,
        },
        loadouts,
    }
}

/// v39: the series tier — every friendly player's abilities on Damage and
/// Healing and the enemies their damage landed on, second by second: what
/// a stored drill windows, read through the same `Segment::series_rows` /
/// `target_series_rows` a live window sums — and (v42) what opens an
/// ability as the live drill does: each Damage ability's targets second by
/// second, every ability's whole-fight targets, and the 1 s damage taken.
/// Built only where it is used — a fight `Retention::wants_series` keeps,
/// or a key's member parsed for a drill — never for a wipe that drops it.
pub fn series_of(seg: &Segment, card: &FightCard) -> FightSeries {
    FightSeries {
        players: card
            .players
            .iter()
            .filter(|p| !p.enemy)
            .map(|p| PlayerSeries {
                guid: p.guid.clone(),
                damage: seg.series_rows(&p.guid, View::Damage),
                heal: seg.series_rows(&p.guid, View::Healing),
                targets: seg.target_series_rows(&p.guid),
                spells: seg.spell_targets_all(&p.guid),
                damage_tallies: seg.spell_tallies_all(&p.guid, View::Damage),
                heal_tallies: seg.spell_tallies_all(&p.guid, View::Healing),
                taken: seg.taken_buckets(&p.guid),
            })
            .filter(|p| !p.is_empty())
            .collect(),
    }
}

/// R19 (step 3b): the drilled player's support block off a rows tier —
/// `None` when the fight wrote none for them (they neither gave nor
/// received), which a PR #19 rows file always reads as.
fn support_of(blocks: &[PlayerSupport], guid: &str) -> Option<PlayerSupport> {
    blocks.iter().find(|s| s.guid == guid).cloned()
}

/// R20 (step 5): the drilled player's shield rows off the rows tier —
/// empty when the fight wrote no block for them.
fn shields_of(blocks: &[PlayerShields], guid: &str) -> Vec<ShieldRow> {
    blocks
        .iter()
        .find(|s| s.guid == guid)
        .map(|s| s.rows.clone())
        .unwrap_or_default()
}

/// R18 (step 4b): `Timeline::coarsen`'s factor over the engine's 1 s grid
/// that yields the rows tier's fixed [`COARSE_BUCKET_MS`].
const COARSE_FACTOR: u32 = COARSE_BUCKET_MS / 1000;
const _: () = assert!(COARSE_FACTOR * 1000 == COARSE_BUCKET_MS);

/// The drilled player's breakdown off the record tiers — the one function
/// `stored_fight` (files) and `derived_fight` (a fresh extract) both call,
/// which is what keeps the two byte-identical. Deaths and Taken answer from
/// the rows tier on every tier the store can serve; Damage and Healing
/// need the details tier for their lists, and the Healing drill's timeline
/// is the details tier's 1 s series when present (tier 3) and the coarse
/// `heal10` otherwise (tier 2, step 4b). The Taken drill's timeline is the
/// coarse taken series with the merged mark list (`bucket_ms` 10 000) —
/// `None` when the player wrote no coarse block.
fn drill_of(
    rows: &FightRows,
    details: Option<&FightDetails>,
    view: View,
    guid: &str,
    death: Option<u32>,
) -> Option<Breakdown> {
    match view {
        // v28 (R9): a stored fight keeps one window per death. `death`
        // picks one; `None` is the last, as it always was. Every answer
        // carries the whole window list so a reader sees the others exist.
        View::Deaths => {
            let mut windows: Vec<&Recap> = rows.recaps.iter().filter(|r| r.guid == guid).collect();
            windows.sort_by_key(|r| r.index);
            // No windows at all means the player did not die here — that is
            // the one "no drill" answer, and it stays `None`. An index that
            // simply names no window is a BAD INDEX, not a missing drill:
            // answer like the live path does, with empty panes, the window
            // list, and no `death_index`, so a caller can tell the two apart.
            if windows.is_empty() {
                return None;
            }
            let picked = match death {
                Some(i) => windows.iter().find(|r| r.index == i).copied(),
                None => windows.last().copied(),
            };
            Some(Breakdown {
                by_spell: picked.map(|r| r.events.clone()).unwrap_or_default(),
                by_target: picked.map(|r| r.attackers.clone()).unwrap_or_default(),
                deaths: windows
                    .iter()
                    .map(|r| DeathWindow {
                        index: r.index,
                        at_ms: r.at_ms,
                    })
                    .collect(),
                death_index: picked.map(|r| r.index),
                // Every window of a player repeats the same count, so a BAD
                // index still reports it — the live path does, and a reader
                // needs it to reconcile against the Deaths row either way.
                deaths_dropped: windows.first().map_or(0, |r| r.dropped),
                range: None,
                ..Breakdown::default()
            })
        }
        View::Damage => {
            let p = details?.players.iter().find(|p| p.guid == guid)?;
            Some(Breakdown {
                by_spell: p.damage_spells.clone(),
                by_target: p.damage_targets.clone(),
                timeline: Some(p.damage_timeline.clone()),
                tree: p.damage_tree.clone(),
                ..Breakdown::default()
            })
        }
        View::Healing => match details {
            // Tier 3: the details tier answers, and it alone — a player
            // the details roster lacks has no Healing drill, exactly as
            // before step 4b; the coarse series is never a substitute for
            // a present-but-silent details file.
            Some(d) => d
                .players
                .iter()
                .find(|p| p.guid == guid)
                .map(|p| Breakdown {
                    by_spell: p.heal_spells.clone(),
                    by_target: p.heal_targets.clone(),
                    timeline: Some(p.heal_timeline.clone()),
                    tree: p.heal_tree.clone(),
                    ..Breakdown::default()
                }),
            // Tier 2 (details demoted): the lists are gone, the coarse
            // series still answers — with the marks.
            None => coarse_of(&rows.coarse, guid).map(|c| Breakdown {
                timeline: Some(c.heal_timeline()),
                ..Breakdown::default()
            }),
        },
        // R17 (step 2b): the Taken drill is answered from the ROWS tier,
        // on every tier the store can serve — the mitigation list is
        // written on every fight, kill or wipe, and the details tier holds
        // no copy of it. `by_target` is the by-attacker list, the spelling
        // every view uses. R18 (step 4b): the timeline is the coarse one.
        View::Taken => rows.mitigation.iter().find(|m| m.guid == guid).map(|m| {
            // R21 (step 6): the stack ledger off the rows tier — empty
            // for a player under no stacking debuff, and on a pre-6
            // rows file.
            let st = stacks_of(&rows.stacks, guid);
            Breakdown {
                by_spell: m.taken_spells.clone(),
                by_target: m.taken_sources.clone(),
                mitigation: Some(m.record),
                timeline: coarse_of(&rows.coarse, guid).map(PlayerCoarse::taken_timeline),
                stacking: st.map(|s| s.debuffs.clone()).unwrap_or_default(),
                stacks: st.map(|s| s.cells.clone()).unwrap_or_default(),
                stacks_dropped: st.map_or(0, |s| s.dropped),
                stack_base: st.map(|s| s.base.clone()).unwrap_or_default(),
                ..Breakdown::default()
            }
        }),
        // v42: the count views' drills off the details tier — the player's
        // by-spell and by-target lists, as the live drill has them (no
        // curve: a count view has none). A details file written before v42
        // keeps none, and neither does a player with nothing on the view.
        View::Interrupts | View::CrowdControl | View::Dispels => details?
            .players
            .iter()
            .find(|p| p.guid == guid)?
            .count(view)
            .map(|c| Breakdown {
                by_spell: c.spells.clone(),
                by_target: c.targets.clone(),
                ..Breakdown::default()
            }),
        _ => None,
    }
}

/// v39: a stored Damage or Healing drill re-answered for a zoom window from
/// the drilled player's seconds — the same rows the live meter folds
/// (`Segment::series_rows`, `target_series_rows`) through the same
/// `series::window_rows`, the window snapped out to whole seconds, so a
/// stored pull's zoom answers what the live one did. The abilities always,
/// the targets where the view keeps them a clock (`View::windows_targets`);
/// the tree keeps its groups alone (`SpellTree::windowed`), and the window
/// is echoed. Every row wears the player's class and spec (`class_spec`).
fn window_drill(
    b: &mut Breakdown,
    p: &PlayerSeries,
    view: View,
    range: (u32, u32),
    (class, spec): (Option<Class>, Option<Spec>),
) {
    let rows = match view {
        View::Damage => &p.damage,
        View::Healing => &p.heal,
        _ => return,
    };
    let w = snap((i64::from(range.0), i64::from(range.1)));
    let secs = (w.1 - w.0).max(0) as f64 / 1000.0;
    b.by_spell = window_rows(rows, Some(w), secs, class, spec);
    if view.windows_targets() {
        b.by_target = window_rows(&p.targets, Some(w), secs, class, spec);
    }
    b.tree = std::mem::take(&mut b.tree).windowed();
    b.range = Some(range);
}

/// v42: the class and spec a player's rows wear in this fight — the card's,
/// which are the meter rows' own.
fn class_spec(card: &FightCard, guid: &str) -> (Option<Class>, Option<Spec>) {
    card.players
        .iter()
        .find(|c| c.guid == guid)
        .map_or((None, None), |c| (c.class, c.spec))
}

/// v42: a curve on the R12 grid wearing the player's marks — the live
/// meter's `marks_for`, which the details tier keeps on each of their
/// timelines.
fn grid(detail: Option<&PlayerDetail>, buckets: Vec<u64>) -> Timeline {
    Timeline {
        bucket_ms: series_model::BUCKET_MS as u32,
        buckets,
        marks: detail.map_or_else(Vec::new, |d| d.damage_timeline.marks.clone()),
    }
}

/// v42: the series tier as one answer reads it — a player's seconds, and
/// whether its file keeps what opens an ability (format 2 on).
#[derive(Clone, Copy)]
struct SeriesTier<'a> {
    read: &'a dyn Fn(&str) -> Option<PlayerSeries>,
    abilities: bool,
}

/// v42: does the drilled player's answer use their seconds — a window, the
/// stack, an opened ability, the 1 s taken curve? Nothing else reads them,
/// so nothing else pays the file's three reads.
fn drill_reads_seconds(ask: &Ask) -> bool {
    let rates = matches!(ask.view, View::Damage | View::Healing);
    (ask.range.is_some() && ask.view.windows_drill())
        || (rates && (ask.stacked || ask.spell.is_some()))
        || ask.view == View::Taken
}

/// v42: does a pair's side use its player's seconds — a window of its
/// tables, the opened ability's curve, the 1 s taken curve?
fn side_reads_seconds(ask: &Ask) -> bool {
    (ask.range.is_some() && crate::engine::compare_windows(ask.view))
        || ask.spell.is_some()
        || ask.view == View::Taken
}

/// v42: one stored answer out of the tiers in hand — the store's files, or
/// a fight just parsed (`Store::derived_fight_in`) — for whatever `ask`
/// asks: the view's rows, the drilled player's lists dressed from their
/// seconds where `series` reads them (`dress`), a pair's comparison from
/// the details tier on (`stored_side`; the drill's lists are then not
/// dressed — a comparison's reader draws the pair), the raid timeline, and
/// the drilled player's support, uptime and shields off the rows tier.
#[allow(clippy::too_many_arguments)]
fn answer(
    mine: &Mine,
    card: FightCard,
    rows_doc: &FightRows,
    details: Option<&FightDetails>,
    tier: u8,
    series: Option<SeriesTier>,
    loadout: Option<wowdps_core::model::Loadout>,
    ask: &Ask,
) -> StoredFight {
    let view = ask.view;
    let drill = ask.drill.as_deref();
    let seconds = |guid: &str| series.and_then(|s| (s.read)(guid));
    let abilities = series.is_some_and(|s| s.abilities);
    let detail = |guid: &str| details.and_then(|d| d.players.iter().find(|p| p.guid == guid));
    let has_recap = drill.is_some_and(|g| rows_doc.recaps.iter().any(|r| r.guid == g));
    let mut rows = rows_doc.rows(view).to_vec();
    let mut breakdown = drill.and_then(|guid| drill_of(rows_doc, details, view, guid, ask.death));
    if let (Some(b), Some(guid)) = (breakdown.as_mut(), drill)
        && ask.pair.is_none()
        && drill_reads_seconds(ask)
        && let Some(p) = seconds(guid)
    {
        dress(b, &p, detail(guid), class_spec(&card, guid), abilities, ask);
    }
    // v44 (R27, R28): the drilled player's resources and pools ride the
    // breakdown as they ride the live one — off the details tier, whatever
    // the view; none below it.
    if let (Some(b), Some(d)) = (breakdown.as_mut(), drill.and_then(detail)) {
        b.energize.clone_from(&d.energize);
        b.power.clone_from(&d.power);
    }
    // v42: a pair compares off the details tier (each side's abilities and
    // curve); the series tier adds its windows and an ability's curve.
    let pair = match (drill, ask.pair.as_deref()) {
        (Some(a), Some(b)) if tier >= 3 => {
            let reads = side_reads_seconds(ask);
            let pa = seconds(a).filter(|_| reads);
            let pb = seconds(b).filter(|_| reads);
            // A window the tables answer only where BOTH sides' seconds
            // read: one side whole beside the other windowed is the
            // mismatch the echo exists to rule out.
            let window = ask
                .range
                .filter(|_| crate::engine::compare_windows(view) && pa.is_some() && pb.is_some());
            let side = |guid: &str, p: Option<&PlayerSeries>| {
                let mut s = stored_side(rows_doc, details, p, &card, guid, window, ask);
                s.total.mine = mine.owns(&s.guid, &s.total.label);
                s
            };
            Some(StoredPair {
                a: side(a, pa.as_ref()),
                b: side(b, pb.as_ref()),
                range: window,
            })
        }
        _ => None,
    };
    // v35 (R25): the pull's raid timeline, rebuilt from the tiers.
    let mut raid = stored_raid(&card, rows_doc, details, view);
    // v35: whose rows are the reader's, said at answer time — never
    // stored, so an alt the addon names later is "you" on old pulls too.
    mark_mine(mine, &card, &mut rows, breakdown.as_mut(), &mut raid);
    // v23 (R19): the drilled player's support block rides from the rows
    // tier whatever the view — `None` when they neither gave nor received
    // (the block is written only for players with support).
    let support = drill.and_then(|guid| support_of(&rows_doc.support, guid));
    let uptime = drill.map_or_else(Vec::new, |guid| uptime_of(&rows_doc.uptime, guid));
    // v26 (R20): the drilled player's shield rows off the rows tier,
    // whatever the view — empty without a drill or for a player who
    // absorbed nothing (a pre-5 rows file always reads as empty).
    let shields = drill.map_or_else(Vec::new, |guid| shields_of(&rows_doc.shields, guid));
    StoredFight {
        card,
        rows,
        breakdown,
        tier,
        has_recap,
        loadout,
        support,
        uptime,
        shields,
        raid: Some(raid),
        series: series.is_some(),
        abilities,
        pair,
        // v43 (R27): the drilled player's resources off the details tier,
        // whatever the view — empty without a drill or below tier 3.
        energize: drill
            .and_then(detail)
            .map_or_else(Vec::new, |d| d.energize.clone()),
        // v44 (R28): their pools second by second, likewise.
        power: drill
            .and_then(detail)
            .map_or_else(Vec::new, |d| d.power.clone()),
    }
}

/// v42: the drilled player's lists dressed from their seconds as the live
/// drill dresses them — a zoom window's lists (v39), the graph's stack (the
/// tree's largest entries, for a client that stacks), an opened ability
/// (its curve; with `abilities`, its targets — windowed on Damage — and
/// their stack), and on Taken the 1 s curve where the rows tier keeps 10 s.
/// A format-1 file keeps no ability's targets (`abilities` false: none are
/// answered, rather than an empty list that says it hit nobody) and no
/// taken seconds (the 10 s curve stands).
fn dress(
    b: &mut Breakdown,
    p: &PlayerSeries,
    detail: Option<&PlayerDetail>,
    who: (Option<Class>, Option<Spec>),
    abilities: bool,
    ask: &Ask,
) {
    let view = ask.view;
    if let Some(range) = ask.range
        && view.windows_drill()
    {
        window_drill(b, p, view, range, who);
    }
    let (class, spec) = who;
    match (view, ask.spell.as_deref()) {
        (View::Damage | View::Healing, None) if ask.stacked => {
            let rows = if view == View::Damage {
                &p.damage
            } else {
                &p.heal
            };
            b.ability_series =
                series_model::stack(&b.by_spell, &b.tree, crate::engine::STACKED, |key| {
                    series_model::curve_of(rows, key)
                });
        }
        (View::Damage, Some(sk)) => {
            b.spell_timeline = Some(grid(detail, series_model::curve_of(&p.damage, sk)));
            if !abilities {
                return;
            }
            let targets = p
                .spells
                .iter()
                .find(|s| s.key == sk)
                .map_or(&[][..], |s| &s.targets[..]);
            b.spell_targets = Some(match ask.range {
                // v38: a Damage window's targets, from the ability's seconds.
                Some(range) => {
                    let w = snap((i64::from(range.0), i64::from(range.1)));
                    let secs = (w.1 - w.0).max(0) as f64 / 1000.0;
                    let mut rows = window_rows(targets, Some(w), secs, class, spec);
                    // An opened ability's targets carry no rate.
                    for r in &mut rows {
                        r.per_sec = 0.0;
                    }
                    rows
                }
                None => tallied(&p.damage_tallies, sk, class, spec),
            });
            if ask.stacked {
                b.target_series = series_model::target_stack(targets, crate::engine::STACKED);
            }
        }
        // Healing keeps no clock for an ability's targets: whole, always.
        (View::Healing, Some(sk)) if abilities => {
            b.spell_targets = Some(tallied(&p.heal_tallies, sk, class, spec));
        }
        (View::Taken, _) if !p.taken.is_empty() && detail.is_some() => {
            b.timeline = Some(grid(detail, p.taken.clone()));
        }
        _ => {}
    }
}

/// v42: an opened ability's whole-fight targets off the series tier's
/// tallies, worded as the live meter words them; none kept, none listed.
fn tallied(
    tallies: &[SpellTallies],
    key: &str,
    class: Option<Class>,
    spec: Option<Spec>,
) -> Vec<Row> {
    tallies
        .iter()
        .find(|t| t.key == key)
        .map_or_else(Vec::new, |t| {
            series_model::target_rows(&t.targets, t.school, class, spec)
        })
}

/// v42: one side of a stored comparison, as `engine::compare_side` builds
/// a live one: the player's meter row (the sum of `window` where one is
/// given — `answer` gives one only when both sides' seconds read), the
/// view's abilities (`drill_of`'s), the curve the view is about — what
/// they healed, what they took (1 s off the series tier, else the rows
/// tier's 10 s), else what they dealt — the opened ability's curve, and on
/// Taken their mitigation record.
fn stored_side(
    rows_doc: &FightRows,
    details: Option<&FightDetails>,
    p: Option<&PlayerSeries>,
    card: &FightCard,
    guid: &str,
    window: Option<(u32, u32)>,
    ask: &Ask,
) -> CompareSide {
    let view = ask.view;
    let detail = details.and_then(|d| d.players.iter().find(|x| x.guid == guid));
    let (total, spells) = match window.zip(p) {
        Some(((lo, hi), p)) => {
            let (class, spec) = class_spec(card, guid);
            let secs = f64::from(hi.saturating_sub(lo)) / 1000.0;
            let series = if view == View::Healing {
                &p.heal
            } else {
                &p.damage
            };
            let spells = window_rows(
                series,
                Some((i64::from(lo), i64::from(hi))),
                secs,
                class,
                spec,
            );
            let label = card
                .players
                .iter()
                .find(|c| c.guid == guid)
                .map_or_else(|| guid.to_string(), |c| c.name.clone());
            let total = Row {
                key: guid.to_string(),
                label,
                class,
                spec,
                ..series_model::total_of(&spells, secs)
            };
            (total, spells)
        }
        None => (
            rows_doc
                .rows(view)
                .iter()
                .find(|r| r.key == guid)
                .cloned()
                .unwrap_or_else(|| Row {
                    key: guid.to_string(),
                    ..Row::default()
                }),
            drill_of(rows_doc, details, view, guid, None)
                .map(|b| b.by_spell)
                .unwrap_or_default(),
        ),
    };
    let timeline = match view {
        View::Healing => detail.map(|d| d.heal_timeline.clone()),
        View::Taken => match p.filter(|p| !p.taken.is_empty()) {
            Some(p) => Some(grid(detail, p.taken.clone())),
            None => coarse_of(&rows_doc.coarse, guid).map(PlayerCoarse::taken_timeline),
        },
        _ => detail.map(|d| d.damage_timeline.clone()),
    }
    .unwrap_or_else(|| grid(detail, Vec::new()));
    CompareSide {
        guid: guid.to_string(),
        total,
        spells,
        timeline,
        // v18: the ability's DAMAGE curve on every view, as live; none for a
        // side that never cast it.
        spell_timeline: ask
            .spell
            .as_deref()
            .zip(p)
            .map(|(sk, p)| grid(detail, series_model::curve_of(&p.damage, sk)))
            .filter(|t| !t.buckets.is_empty()),
        mitigation: (view == View::Taken)
            .then(|| {
                rows_doc
                    .mitigation
                    .iter()
                    .find(|m| m.guid == guid)
                    .map(|m| m.record)
            })
            .flatten(),
    }
}

/// v35: mark a stored answer's rows `mine` — the meter's by guid, a drill's
/// player-naming lists by the names `card` gives the account's characters,
/// the raid timeline's deaths by guid. Answer time only: `row_json` never
/// writes the flag.
fn mark_mine(
    mine: &Mine,
    card: &FightCard,
    rows: &mut [Row],
    breakdown: Option<&mut Breakdown>,
    raid: &mut wowdps_model::RaidTimeline,
) {
    mine.mark_rows(rows);
    mine.mark_raid(raid);
    if let Some(b) = breakdown {
        let here = mine.names_among(
            card.players
                .iter()
                .map(|p| (p.guid.as_str(), p.name.as_str())),
        );
        mine.mark_breakdown(b, false, &here);
    }
}

/// R25 (v35): a stored pull's raid timeline, rebuilt from what the store
/// keeps — so a pull of an earlier night opens with the ribbon, the
/// chronological Deaths table and the stat line's deaths a live pull has:
///
/// - the DEATHS from the rows tier's recaps (R9: one per window, its killing
///   blow the newest hit — the recap's own first damage row), each player's
///   name, class, spec and team off the card, the RESURRECTION off their
///   R23 death mark of the same moment in the coarse mark list (its label
///   "Death (Spell)", its caster the rezzer — none named is a self-rez);
/// - the LUST windows off the same lists' External marks of the lust
///   family, unioned exactly as the live meter unions them;
/// - the view's SERIES (`View::raid_series`): the details tier's 1 s damage
///   or healing when it is on disk, else the coarse 10 s taken or healing;
///   Damage with its details demoted has none, and the reader draws the
///   axis, the lust and the deaths alone.
///
/// Friendly players only for the series and the lust, as live; an arena's
/// hostile deaths are kept and flagged `enemy`.
fn stored_raid(
    card: &FightCard,
    rows: &FightRows,
    details: Option<&FightDetails>,
    view: View,
) -> wowdps_model::RaidTimeline {
    use wowdps_model::{MarkKind, RaidDeath, RaidTimeline, Rez};
    let player = |guid: &str| card.players.iter().find(|p| p.guid == guid);
    let name_of = |guid: &str| player(guid).map_or_else(|| guid.to_string(), |p| p.name.clone());
    let friendly = |guid: &str| player(guid).is_some_and(|p| !p.enemy);
    let series_view = view.raid_series();
    let sum = |lists: &mut dyn Iterator<Item = &Vec<u64>>| {
        let mut out: Vec<u64> = Vec::new();
        for s in lists {
            if out.len() < s.len() {
                out.resize(s.len(), 0);
            }
            for (slot, v) in out.iter_mut().zip(s) {
                *slot += v;
            }
        }
        out
    };
    let friendly_details = || {
        details
            .into_iter()
            .flat_map(|d| d.players.iter())
            .filter(|p| friendly(&p.guid))
    };
    let friendly_coarse = || rows.coarse.iter().filter(|c| friendly(&c.guid));
    let (bucket_ms, series) = match (series_view, details) {
        (View::Healing, Some(_)) => (
            1000,
            sum(&mut friendly_details().map(|p| &p.heal_timeline.buckets)),
        ),
        (View::Healing, None) => (
            COARSE_BUCKET_MS,
            sum(&mut friendly_coarse().map(|c| &c.heal10)),
        ),
        (View::Taken, _) => (
            COARSE_BUCKET_MS,
            sum(&mut friendly_coarse().map(|c| &c.taken10)),
        ),
        (_, Some(_)) => (
            1000,
            sum(&mut friendly_details().map(|p| &p.damage_timeline.buckets)),
        ),
        (_, None) => (1000, Vec::new()),
    };
    let marks_of = |guid: &str| -> Vec<&wowdps_model::Mark> {
        rows.coarse
            .iter()
            .filter(|c| c.guid == guid)
            .flat_map(|c| c.marks.iter())
            .collect()
    };
    let mut deaths: Vec<RaidDeath> = rows
        .recaps
        .iter()
        .map(|r| {
            // Newest first: the first damage is the killing blow, its label
            // "Spell (Source)" as the recap words it (the spell alone for a
            // nil source). The window's attackers are keyed by the source's
            // name — or by the spell's for a nil source — so a split is
            // taken only when its source is one of them and the whole label
            // is not: a nil-source ability whose own name ends in a
            // parenthetical ("Blight (Heroic)") stays whole, as live has it.
            // A recap with no attackers kept (none written) splits as said.
            let attacker = |name: &str| r.attackers.iter().any(|a| a.label == name);
            let known = !r.attackers.is_empty();
            let blow = r.events.iter().find(|e| !e.gain);
            let (spell, source) = blow.map_or((String::new(), String::new()), |e| {
                match e
                    .label
                    .strip_suffix(')')
                    .and_then(|rest| rest.rsplit_once(" ("))
                {
                    Some((s, src))
                        if !s.is_empty()
                            && !src.is_empty()
                            && (!known || (attacker(src) && !attacker(&e.label))) =>
                    {
                        (s.to_string(), src.to_string())
                    }
                    _ => (e.label.clone(), String::new()),
                }
            });
            let rez = marks_of(&r.guid)
                .into_iter()
                .find(|m| m.kind == MarkKind::Death && m.at_ms == r.at_ms)
                .and_then(|m| {
                    let spell = m.label.strip_prefix("Death (")?.strip_suffix(')')?;
                    let by = if m.src.is_empty() {
                        r.guid.clone()
                    } else {
                        m.src.clone()
                    };
                    Some(Rez {
                        at_ms: m.at_ms + m.dur_ms,
                        by_name: name_of(&by),
                        by,
                        spell: spell.to_string(),
                    })
                });
            let p = player(&r.guid);
            RaidDeath {
                guid: r.guid.clone(),
                name: name_of(&r.guid),
                class: p
                    .and_then(|p| p.class)
                    .or_else(|| blow.and_then(|e| e.class)),
                spec: p.and_then(|p| p.spec).or_else(|| blow.and_then(|e| e.spec)),
                index: r.index,
                at_ms: r.at_ms,
                blow: spell,
                source,
                hit: blow.map_or(0, |e| e.amount),
                overkill: blow.map(|e| e.extra).filter(|o| *o > 0),
                rez,
                mine: false,
                enemy: p.is_some_and(|p| p.enemy),
            }
        })
        .collect();
    deaths.sort_by(|a, b| (a.at_ms, &a.guid, a.index).cmp(&(b.at_ms, &b.guid, b.index)));
    let lust = wowdps_core::meter::lust_windows(
        friendly_coarse()
            .flat_map(|c| c.marks.iter())
            .filter(|m| m.kind == MarkKind::External && wowdps_core::meter::is_lust(m.spell_id))
            .map(|m| (m.at_ms, m.dur_ms, m.label.as_str())),
    );
    RaidTimeline {
        view: series_view,
        bucket_ms,
        series,
        deaths,
        lust,
    }
}

/// R21 (step 6): the drilled player's stack block off the rows tier.
fn stacks_of<'a>(blocks: &'a [PlayerStacks], guid: &str) -> Option<&'a PlayerStacks> {
    blocks.iter().find(|s| s.guid == guid)
}

fn coarse_of<'a>(blocks: &'a [PlayerCoarse], guid: &str) -> Option<&'a PlayerCoarse> {
    blocks.iter().find(|c| c.guid == guid)
}

/// R18 (step 4b): the drilled player's uptime over the wire — BOTH halves:
/// every cell of their own block (they are the target; a self-cast lives
/// here and nowhere else), in the engine's order, then every cell on any
/// OTHER block whose `src` is the player (they cast it — "externals given,
/// to whom", a supporter's per-target uptime), blocks in roster order.
fn uptime_of(blocks: &[PlayerUptime], guid: &str) -> Vec<StoredUptime> {
    let own = blocks
        .iter()
        .filter(|b| b.guid == guid)
        .flat_map(|b| b.cells.iter());
    let cast = blocks.iter().filter(|b| b.guid != guid).flat_map(|b| {
        b.cells
            .iter()
            .filter(|c| c.src == guid)
            .map(move |c| (b, c))
    });
    own.map(|c| StoredUptime {
        target: guid.to_string(),
        cell: c.clone(),
    })
    .chain(cast.map(|(b, c)| StoredUptime {
        target: b.guid.clone(),
        cell: c.clone(),
    }))
    .collect()
}

/// R17 (step 2b): a Taken drill list as the rows tier keeps it — by
/// ability or by attacker — sorted by amount descending (a stable sort, so
/// the meter's own label tie-break survives), the first `TAKEN_SPELLS_CAP`
/// kept and the rest folded into one `TakenOther`. Identity: Σ kept
/// `amount` / `extra` / `count` + the fold = the player's Taken row. On a
/// boss pull (~9 abilities, ~5 attackers) nothing folds and `n` is 0; the
/// cap bites Σ records — and the attacker list of a raid night's Overall
/// hardest (74 names on one player, measured in
/// `docs/plan-role-pivots-step2b.md`).
fn cap_taken(mut spells: Vec<Row>) -> (Vec<Row>, TakenOther) {
    spells.sort_by_key(|r| std::cmp::Reverse(r.amount));
    let rest = if spells.len() > TAKEN_SPELLS_CAP {
        spells.split_off(TAKEN_SPELLS_CAP)
    } else {
        Vec::new()
    };
    let other = TakenOther {
        amount: rest.iter().map(|r| r.amount).sum(),
        extra: rest.iter().map(|r| r.extra).sum(),
        count: rest.iter().map(|r| r.count).sum(),
        n: u32::try_from(rest.len()).unwrap_or(u32::MAX),
    };
    (spells, other)
}

/// Does a configured "Name-Realm" (or bare "Name") name this player?
pub(crate) fn name_matches(wanted: &[String], name: &str) -> bool {
    let full = name.to_lowercase();
    let bare = full.split('-').next().unwrap_or(&full);
    wanted
        .iter()
        .any(|w| w == &full || (!w.contains('-') && w == bare))
}
