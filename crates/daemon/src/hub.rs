//! The hub thread: owns the engines (one per followed source, `Sources`)
//! and the session table, applies client messages, and on each tick
//! rebuilds snapshots for watched cursors only, pushing the ones that
//! changed. Snapshot rate is capped by the tick.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender};
use std::time::{Duration, Instant};

use wowdps_core::index::SegmentMeta;
use wowdps_core::model::{Meter, SegmentId};
use wowdps_core::tail::TailEvent;
use wowdps_proto::{
    ClientKind, ClientMsg, Cursor, DaemonMsg, EditionRef, HistoryAnswer, HistoryQuery, LoadError,
    Night, PROTO_VERSION, SegmentRef,
};

use crate::engine::{Built, Engine, EngineEvent, LoadoutBuilt};
use crate::game::Running;
use crate::history::{Ask, HistoryLink, HistoryReq, LogRef};
use crate::loader::{LoadReply, LoadReq};
use crate::overlay::{Cmd, Supervisor};
use crate::session::Session;
use crate::sources::{SourceSetup, Sources, cursor_id};

pub enum HubMsg {
    /// v49: one followed source's tail event, by the source's place in
    /// `HubOptions::sources` (0 is the first).
    Tail(usize, TailEvent),
    Connected {
        id: u64,
        kind: ClientKind,
        pid: u32,
        tx: SyncSender<DaemonMsg>,
    },
    Client {
        id: u64,
        msg: ClientMsg,
    },
    Disconnected {
        id: u64,
    },
    Loaded {
        id: SegmentId,
        /// Boxed for the same reason as `TailEvent::Index`: a `Meter` is ~320
        /// bytes and would set the size of every message the hub receives.
        result: Result<Box<Meter>, String>,
    },
    /// v49: the game watcher's word — which editions' clients run.
    Game(Running),
    /// v20: the history thread's reply to one session's one-shot.
    History {
        session: u64,
        /// Boxed like `Loaded`: a `Fights` answer can carry many cards.
        msg: Box<DaemonMsg>,
    },
    /// v20: the store wrote (or pinned) a fight — broadcast to every session.
    HistoryChanged {
        fight_id: String,
    },
}

pub struct HubOptions {
    pub linger: bool,
    pub idle_grace: Duration,
    pub tick: Duration,
    pub version: String,
    /// v49: every followed source, in order: the first is what `--file`,
    /// `--logs` or the config names, and its canonical display
    /// (`spec_display`) is `Status::source`, so a client with
    /// `--file`/`--logs` can detect a conflict. Empty is one source naming
    /// nothing.
    pub sources: Vec<SourceSetup>,
}

impl HubOptions {
    /// What `Status::source` reports: the first source's display.
    fn source_spec(&self) -> Option<String> {
        self.sources.first().and_then(|s| s.spec.clone())
    }
}

/// Runs until `Shutdown`, idle-exit, or every sender hanging up.
pub fn run(
    rx: Receiver<HubMsg>,
    loader: Sender<LoadReq>,
    mut supervisor: Supervisor,
    opts: HubOptions,
    history: HistoryLink,
) {
    let mut sources = Sources::new(&opts.sources);
    let mut sessions: Vec<Session> = Vec::new();
    let mut shutdown = false;
    let mut idle_since: Option<Instant> = Some(Instant::now());
    let mut last_tick = Instant::now();

    while !shutdown {
        let timeout = opts.tick.saturating_sub(last_tick.elapsed());
        // v35: whose rows are the reader's, as the history thread last
        // resolved it — a refcount, read before anything is built.
        let mine = history.mine();
        for engine in sources.engines_mut() {
            engine.mine = Arc::clone(&mine);
        }
        match rx.recv_timeout(timeout) {
            Ok(msg) => handle(
                msg,
                &mut sources,
                &mut sessions,
                &loader,
                &mut supervisor,
                &opts,
                &mut shutdown,
                &history,
            ),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if last_tick.elapsed() >= opts.tick {
            last_tick = Instant::now();
            tick(&mut sources, &mut sessions, &loader);
            let cmds = supervisor.on_tick();
            deliver(&mut sessions, cmds);
        }

        let disconnected: Vec<u64> = sessions.iter().filter(|s| s.dead).map(|s| s.id).collect();
        for id in disconnected {
            supervisor.on_overlay_disconnected(id);
        }
        sessions.retain(|s| !s.dead);

        // Idle-exit: a watching session, the overlay supervisor (live child /
        // mid-exit-grace) or history imports still queued hold the daemon
        // open — a one-shot `wowdps history import` must not spawn a daemon
        // that quits with most of its jobs undispatched.
        let holding = sessions.iter().any(|s| s.cursor.is_some())
            || supervisor.holds_daemon_open()
            || (history.enabled() && history.status().importing > 0);
        if holding {
            idle_since = None;
        } else if idle_since.is_none() {
            idle_since = Some(Instant::now());
        }
        if !opts.linger
            && let Some(t) = idle_since
            && t.elapsed() >= opts.idle_grace
        {
            break;
        }
    }
    // Every way out takes down the overlay this daemon spawned: an orphan's
    // reconnect respawns a daemon, and that daemon spawns another overlay.
    // A panic unwinding out of here skips this line; the supervisor's `Drop`
    // does the same then.
    supervisor.shutdown();
    // Dropping the sessions' senders lets every writer thread run down,
    // which shuts each stream and unblocks its reader.
}

/// Hand a session's history one-shot to the history thread. The protocol
/// promises every such request a reply, so when there is no store, or the
/// (deliberately lossy) queue is full and `send` hands the request back,
/// the session gets the empty answer here instead of silence.
fn forward_history(history: &HistoryLink, s: &mut Session, req: HistoryReq) {
    let rejected = if history.enabled() {
        match history.send(req) {
            Ok(()) => return,
            Err(req) => req,
        }
    } else {
        req
    };
    let msg = match rejected {
        HistoryReq::Query { req_id, query, .. } => {
            let answer = match query {
                HistoryQuery::Fights { .. } => HistoryAnswer::Fights {
                    cards: Vec::new(),
                    total: 0,
                },
                HistoryQuery::Progression { .. } => HistoryAnswer::Progression {
                    pulls: 0,
                    kills: 0,
                    first_kill: None,
                    nights: Vec::new(),
                    median_kill_ms: None,
                },
                HistoryQuery::Trend { .. } => HistoryAnswer::Trend(Vec::new()),
                HistoryQuery::RoleNight { night, .. } => HistoryAnswer::RoleNight {
                    night: Night {
                        day_utc_ms: night,
                        pulls: 0,
                        kill: false,
                        kills: 0,
                        best_pct: None,
                        tz_min: None,
                    },
                    rows: Vec::new(),
                },
            };
            DaemonMsg::History { req_id, answer }
        }
        HistoryReq::Fight { req_id, .. } => DaemonMsg::Fight {
            req_id,
            fight: None,
        },
        HistoryReq::Pin {
            req_id, fight_id, ..
        } => DaemonMsg::History {
            req_id,
            answer: HistoryAnswer::Pinned {
                fight_id,
                pinned: false,
            },
        },
        HistoryReq::ImportLog { req_id, .. } => DaemonMsg::History {
            req_id,
            answer: HistoryAnswer::Imported { queued: 0 },
        },
        HistoryReq::Regrade { req_id, .. } => DaemonMsg::History {
            req_id,
            answer: HistoryAnswer::Regraded { queued: 0 },
        },
        HistoryReq::Replay {
            req_id, fight_id, ..
        } => DaemonMsg::Replay {
            req_id,
            fight_id,
            bytes: None,
        },
        // v46: a write the store did not take says why — a store that is
        // off, or a queue full of closing pulls.
        HistoryReq::ImportReplay { req_id, .. } => DaemonMsg::History {
            req_id,
            answer: HistoryAnswer::Refused {
                reason: if history.enabled() {
                    "the history store is busy; try again".to_string()
                } else {
                    history
                        .status()
                        .error
                        .unwrap_or_else(|| "the history store is off".to_string())
                },
            },
        },
        // Engine-side forwards are lossy by design and answer nobody.
        HistoryReq::Store(_)
        | HistoryReq::Index { .. }
        | HistoryReq::Sweep(_)
        | HistoryReq::Retire(_)
        | HistoryReq::Loaded { .. } => return,
    };
    s.push_control(msg);
}

/// Send supervisor commands to the overlay session, if one is connected.
fn deliver(sessions: &mut [Session], cmds: Vec<Cmd>) {
    for cmd in cmds {
        let Cmd::SetVisible(v) = cmd;
        for s in sessions
            .iter_mut()
            .filter(|s| s.kind == ClientKind::Overlay)
        {
            s.push_control(DaemonMsg::SetVisible(v));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle(
    msg: HubMsg,
    sources: &mut Sources,
    sessions: &mut Vec<Session>,
    loader: &Sender<LoadReq>,
    supervisor: &mut Supervisor,
    opts: &HubOptions,
    shutdown: &mut bool,
    history: &HistoryLink,
) {
    match msg {
        HubMsg::Tail(i, ev) => {
            if i >= sources.len() {
                return;
            }
            let engine = sources.engine(i);
            // A switch to a newer log retires the one we leave: its open
            // tail (an abandoned pull, the raid night's Σ) is final now and
            // imports as it would on a restart, so a lingering daemon never
            // waits for one. The first `Switched` has nothing to retire.
            // v49: each source retires its own.
            if history.enabled()
                && let TailEvent::Switched(new) = &ev
                && let Some(old) = engine.source_path.clone()
                && &old != new
            {
                let _ = history.send(HistoryReq::Retire(old));
            }
            // The tailed log's index goes to the history store too: its
            // closed segments are backlog, imported off this thread.
            if history.enabled()
                && let TailEvent::Index { index, .. } = &ev
                && let Some(path) = engine.source_path.clone()
            {
                let _ = history.send(HistoryReq::Index {
                    log: LogRef { path },
                    segments: index.segments.clone(),
                    overalls: index.overalls.clone(),
                });
            }
            let awaiting = engine.awaiting_index();
            let mut events = Vec::new();
            let named = sources.on_tail(i, ev, &mut events);
            // The live edition is chosen on the tick (`tick`), the lines
            // landed within it judged together; here it stands.
            for ev in events {
                match ev {
                    // v49: to the sessions this source answers — a pin to
                    // another edition has no fight starting here, and a
                    // follower none but the live edition's.
                    EngineEvent::Opened(id) => {
                        let engine = sources.engine(i);
                        for s in sessions.iter_mut().filter(|s| {
                            sources.for_scope(s.edition) == i && engine.serves(s.edition)
                        }) {
                            s.push_control(DaemonMsg::SegmentOpened { id });
                        }
                    }
                    // One clone and a try_send: every other byte of history
                    // work happens on its own thread.
                    EngineEvent::Closed(id) => {
                        if !history.enabled() {
                            continue;
                        }
                        let engine = sources.engine_mut(i);
                        if let Some(fight) = engine.take_closed(id) {
                            let _ = history.send(HistoryReq::Store(Box::new(fight)));
                        } else if let Some(meta) = engine.closed_needs_prefix(id) {
                            // A keyed visit's Σ is its only record, and the
                            // file will not close it before a restart: parse
                            // the prefix now and store when it lands.
                            engine.history_pending.insert(id);
                            request_load(engine, loader, id, meta);
                        }
                    }
                }
            }
            // The id table changed shape: broadcast the list to every
            // session this source answers, not just List watchers.
            // Off-list navigation resolves neighbors by id, and a segment
            // that opened *and closed* inside one flush burst never gets a
            // `SegmentOpened` of its own (`Opened` only covers a batch's
            // still-open tail). v49: so does a change of the source's
            // edition with no id moving (`ListKey`). And a log switched to
            // is served to pinned sessions only once its index (or a line)
            // lands (`Engine::serves`): they are sent the list then, whether
            // its key moved or not — an empty, header-less new log's has
            // not.
            let key = sources.engine(i).list_key();
            let changed = match sources.source_mut(i) {
                Some(src) if src.last_list != key => {
                    src.last_list = key;
                    true
                }
                _ => false,
            };
            if changed {
                broadcast_list(sources, sessions, i, false);
            } else if awaiting && !sources.engine(i).awaiting_index() {
                broadcast_list(sources, sessions, i, true);
            }
            // v49: the sources were just named by their folders (a second
            // one's first log): another source's edition moves where its
            // log named none yet, and its list goes out as its own would.
            if named {
                for j in (0..sources.len()).filter(|j| *j != i) {
                    let key = sources.engine(j).list_key();
                    if let Some(src) = sources.source_mut(j)
                        && src.last_list != key
                    {
                        src.last_list = key;
                        broadcast_list(sources, sessions, j, false);
                    }
                }
            }
            // v49: every session now answered by another source than the
            // one whose list it holds — the followers of a live edition that
            // moved, a pin whose edition another source now serves — is sent
            // that one's list, which moves it.
            resync(sources, sessions);
        }
        HubMsg::Connected { id, kind, pid, tx } => {
            let mut s = Session::new(id, kind, pid, tx);
            s.push_control(DaemonMsg::HelloAck {
                proto: PROTO_VERSION,
                version: opts.version.clone(),
            });
            sessions.push(s);
            if kind == ClientKind::Overlay {
                let cmds = supervisor.on_overlay_connected(id);
                deliver(sessions, cmds);
            }
        }
        HubMsg::Client { id, msg } => {
            if let ClientMsg::Shutdown = msg {
                *shutdown = true;
                return;
            }
            let Some(s) = sessions.iter_mut().find(|s| s.id == id) else {
                return;
            };
            match msg {
                ClientMsg::Watch { cursor, edition } => {
                    s.set_cursor(cursor, edition);
                    // Answer immediately: a view change or drilldown open is
                    // one round trip, not one round trip plus a tick.
                    push_cursor(s, sources, loader);
                }
                ClientMsg::GetStatus { req_id } => {
                    let clients = sessions.len() as u32;
                    let source = opts.source_spec();
                    let overlay = supervisor.state();
                    let history = history.status();
                    // v49: every edition's source, the live one marked.
                    let editions = sources.edition_sources();
                    let game_running = sources.running().any();
                    let Some(s) = sessions.iter_mut().find(|s| s.id == id) else {
                        return;
                    };
                    s.push_control(DaemonMsg::Status {
                        req_id,
                        game_running,
                        source,
                        clients,
                        linger: opts.linger,
                        overlay,
                        history,
                        editions,
                    });
                }
                ClientMsg::VisibilityChanged { visible } => {
                    s.visible = visible;
                    if s.kind == ClientKind::Overlay {
                        supervisor.on_visibility_changed(visible);
                    }
                }
                // R11: the trash can, in the source serving the session.
                // The id table shrinks: broadcast the new list to every
                // session it answers, exactly like a tail change. v49: a
                // session pinned to an edition no source serves holds an
                // empty list, and its trash can tombstones nothing.
                ClientMsg::DiscardTrash => {
                    let Some(i) = sources.serving(s.edition) else {
                        return;
                    };
                    sources.engine_mut(i).discard_trash();
                    let key = sources.engine(i).list_key();
                    if let Some(src) = sources.source_mut(i)
                        && src.last_list != key
                    {
                        src.last_list = key;
                        broadcast_list(sources, sessions, i, false);
                    }
                }
                // v19: a one-shot, answered like GetStatus when the segment
                // is warm; a cold segment parks the request behind the
                // loader and `HubMsg::Loaded` answers it. v49: an id in the
                // source holding it, `Live` in the session's edition's; an
                // id no source holds is nobody's.
                ClientMsg::GetLoadout {
                    req_id,
                    segment,
                    guid,
                } => {
                    let i = match segment {
                        SegmentRef::Id(seg) => sources.owner_of(seg),
                        SegmentRef::Live => sources.serving(s.edition),
                    };
                    let built = match i {
                        Some(i) => sources.engine_mut(i).loadout_in(s.edition, segment, &guid),
                        None => LoadoutBuilt::Ready(None),
                    };
                    match built {
                        LoadoutBuilt::Ready(loadout) => {
                            s.push_control(DaemonMsg::Loadout {
                                req_id,
                                guid,
                                loadout,
                            });
                        }
                        LoadoutBuilt::Loading(seg_id, meta) => {
                            s.pending_loadouts.push((req_id, seg_id, guid));
                            let engine = sources.engine_mut(i.unwrap_or(0));
                            request_load(engine, loader, seg_id, meta);
                        }
                    }
                }
                // v20: history one-shots go to the history thread with the
                // session id; the reply comes back as `HubMsg::History`. A
                // disabled store — or a full queue — answers empty right
                // here, never an error.
                ClientMsg::GetHistory { req_id, query } => {
                    forward_history(
                        history,
                        s,
                        HistoryReq::Query {
                            session: id,
                            req_id,
                            query,
                        },
                    );
                }
                ClientMsg::GetFight {
                    req_id,
                    fight_id,
                    view,
                    drill,
                    death,
                    boss,
                    range,
                    spell,
                    pair,
                } => {
                    // v42: the stack is built for a client that draws it,
                    // as a live drill's is.
                    let stacked = crate::engine::wants_series(s.kind);
                    forward_history(
                        history,
                        s,
                        HistoryReq::Fight {
                            session: id,
                            req_id,
                            fight_id,
                            boss,
                            ask: Box::new(Ask {
                                view,
                                drill,
                                death,
                                range,
                                spell,
                                pair,
                                stacked,
                            }),
                        },
                    );
                }
                // v45 (R29): a stored fight's replay tier, read on the
                // history thread (a read, under its quota).
                ClientMsg::GetReplay { req_id, fight_id } => {
                    forward_history(
                        history,
                        s,
                        HistoryReq::Replay {
                            session: id,
                            req_id,
                            fight_id,
                        },
                    );
                }
                ClientMsg::PinFight {
                    req_id,
                    fight_id,
                    pinned,
                } => {
                    forward_history(
                        history,
                        s,
                        HistoryReq::Pin {
                            session: id,
                            req_id,
                            fight_id,
                            pinned,
                        },
                    );
                }
                ClientMsg::ImportLog { req_id, path } => {
                    forward_history(
                        history,
                        s,
                        HistoryReq::ImportLog {
                            session: id,
                            req_id,
                            path: std::path::PathBuf::from(path),
                        },
                    );
                }
                ClientMsg::Regrade {
                    req_id,
                    fight_id,
                    encounter,
                    difficulty,
                    kind,
                    edition,
                } => {
                    forward_history(
                        history,
                        s,
                        HistoryReq::Regrade {
                            session: id,
                            req_id,
                            fight_id,
                            encounter,
                            difficulty,
                            kind,
                            edition,
                        },
                    );
                }
                // v46: a fight from outside this machine's logs, stored by
                // the history thread (the store's one writer).
                ClientMsg::ImportReplay {
                    req_id,
                    card,
                    replay,
                } => {
                    forward_history(
                        history,
                        s,
                        HistoryReq::ImportReplay {
                            session: id,
                            req_id,
                            card,
                            replay,
                        },
                    );
                }
                ClientMsg::Hello { .. } | ClientMsg::Shutdown => {}
            }
        }
        HubMsg::Disconnected { id } => {
            supervisor.on_overlay_disconnected(id);
            sessions.retain(|s| s.id != id);
        }
        HubMsg::Loaded { id, result } => {
            // v49: to the source that asked for it. A slice whose log
            // rotated away while it was parsed is no source's: its meter is
            // dropped, never installed where no id resolves to it (one
            // source installed it all the same, an LRU slot spent on a meter
            // nothing could read) — its waiters are answered as before.
            let claimed = sources.for_load(id);
            let loaded_ok = result.is_ok();
            match result {
                Ok(meter) => {
                    if let Some(i) = claimed {
                        let engine = sources.engine_mut(i);
                        engine.loading.remove(&id);
                        engine.install_loaded(id, *meter);
                        if engine.history_pending.remove(&id)
                            && history.enabled()
                            && let Some(fight) = engine.take_closed(id)
                        {
                            let _ = history.send(HistoryReq::Store(Box::new(fight)));
                        }
                    }
                    // Whoever is waiting on this segment gets it now, not at
                    // the next tick.
                    for s in sessions.iter_mut() {
                        if cursor_wants(s.cursor.as_ref(), id) {
                            push_cursor(s, sources, loader);
                        }
                    }
                }
                Err(e) => {
                    // The sweep after the next start stores it from the file.
                    if let Some(i) = claimed {
                        let engine = sources.engine_mut(i);
                        engine.loading.remove(&id);
                        engine.history_pending.remove(&id);
                    }
                    for s in sessions.iter_mut() {
                        if cursor_wants(s.cursor.as_ref(), id) && s.last_load_error != Some(id) {
                            s.last_load_error = Some(id);
                            s.push_control(DaemonMsg::LoadFailed {
                                segment: id,
                                error: LoadError::Io(e.clone()),
                            });
                        }
                    }
                }
            }
            // v19: parked one-shots are ALWAYS answered — from the resident
            // meter on success, with None on a failed load.
            for s in sessions.iter_mut() {
                let (matching, rest) = std::mem::take(&mut s.pending_loadouts)
                    .into_iter()
                    .partition(|(_, sid, _)| *sid == id);
                s.pending_loadouts = rest;
                for (req_id, sid, guid) in matching {
                    let answer =
                        claimed.map(|i| sources.engine_mut(i).loadout(SegmentRef::Id(sid), &guid));
                    let loadout = match (loaded_ok, answer) {
                        (true, Some(LoadoutBuilt::Ready(l))) => l,
                        _ => None,
                    };
                    s.push_control(DaemonMsg::Loadout {
                        req_id,
                        guid,
                        loadout,
                    });
                }
            }
        }
        HubMsg::Game(running) => {
            // v49: which editions run: each source's liveness follows its
            // own game, and the live edition the running game's.
            sources.set_running(running);
            sources.choose_live();
            resync(sources, sessions);
            // The overlay is shown while any game runs, as it always was.
            let cmds = supervisor.on_game(sources.running().any());
            deliver(sessions, cmds);
        }
        // v20: a one-shot's answer, with control-message ordering; the
        // session may have left meanwhile, which is fine.
        HubMsg::History { session, msg } => {
            if let Some(s) = sessions.iter_mut().find(|s| s.id == session) {
                s.push_control(*msg);
            }
        }
        HubMsg::HistoryChanged { fight_id } => {
            for s in sessions.iter_mut() {
                s.push_control(DaemonMsg::HistoryChanged {
                    fight_id: fight_id.clone(),
                });
            }
        }
    }
}

/// The tick: the live edition chosen by its rule (`Sources::choose_live`)
/// over every line landed since the last tick — one source is always live,
/// and moves nothing — then every session's cursor rebuilt and pushed when
/// changed.
fn tick(sources: &mut Sources, sessions: &mut [Session], loader: &Sender<LoadReq>) {
    if sources.choose_live() {
        resync(sources, sessions);
    }
    for s in sessions.iter_mut() {
        push_cursor(s, sources, loader);
    }
}

/// Source `i`'s list changed (its ids, or v49 the edition its source
/// names): the list to every session it answers (`Sources::for_scope`),
/// not just List watchers, each as its edition answers it
/// (`Engine::list_for`) — this source's list stamped with the edition it is
/// answered in, or, to a session pinned to an edition this source is not
/// (and no other source is), that edition's empty list: one that stops
/// being served is emptied then, never left holding another edition's ids.
/// With `pinned_only`, to pinned sessions alone (a following one's list did
/// not change).
fn broadcast_list(sources: &Sources, sessions: &mut [Session], i: usize, pinned_only: bool) {
    let engine = sources.engine(i);
    let list = engine.build_list(engine.game_running);
    for s in sessions
        .iter_mut()
        .filter(|s| !pinned_only || s.edition != EditionRef::Follow)
        .filter(|s| sources.for_scope(s.edition) == i)
    {
        s.push_list(engine.list_for(s.edition, &list));
        s.answered_by = Some(i);
    }
}

/// v49: a session holding one source's list but now answered by another —
/// the live edition moved under a follower, or a pin's edition is another
/// source's now — is sent that source's list as it answers it: the frame
/// that moves the client (another edition, or another log). A daemon of
/// one source never sends one.
fn resync(sources: &Sources, sessions: &mut [Session]) {
    for s in sessions.iter_mut() {
        let i = sources.for_scope(s.edition);
        if s.answered_by.is_some_and(|was| was != i) {
            let engine = sources.engine(i);
            let list = engine.build_list(engine.game_running);
            let msg = engine.list_for(s.edition, &list);
            // A list the session holds already (an unserved pin's empty
            // one, the same from every source) moves nothing: not re-sent.
            if !s.holds_empty_list(&msg) {
                s.push_list(msg);
            }
            s.answered_by = Some(i);
        }
    }
}

fn cursor_wants(cursor: Option<&Cursor>, id: SegmentId) -> bool {
    // R12: a comparison waits on its segment's slice exactly like a meter.
    let watching = match cursor {
        Some(Cursor::Segment { segment, .. }) | Some(Cursor::Compare { segment, .. }) => *segment,
        _ => return false,
    };
    matches!(watching, wowdps_proto::SegmentRef::Id(i) if i == id)
}

/// Hand a cold segment to the loader pool exactly once — `engine.loading`
/// dedups across sessions and ticks. One copy of the gate for every path
/// that can trigger a load (meter, compare, loadout), so the discipline
/// cannot diverge. No source path means a load can never be serviced; the
/// waiter stays parked (snapshots keep their placeholder, a one-shot until
/// its session disconnects).
fn request_load(engine: &mut Engine, loader: &Sender<LoadReq>, id: SegmentId, meta: SegmentMeta) {
    if !engine.loading.contains(&id)
        && let Some(path) = engine.source_path.clone()
    {
        engine.loading.insert(id);
        let _ = loader.send(LoadReq {
            id,
            path,
            meta,
            reply: LoadReply::Hub,
        });
    }
}

/// Build and (dedup-)push whatever `s` is watching. v49: from the source
/// that answers it — the one holding a watched segment id (an id no source
/// holds fails: rotated away, or never issued), else the session's
/// edition's (`Sources::for_scope`).
fn push_cursor(s: &mut Session, sources: &mut Sources, loader: &Sender<LoadReq>) {
    let Some(cursor) = s.cursor.as_ref() else {
        return;
    };
    let i = match sources.for_cursor(cursor, s.edition) {
        Ok(i) => i,
        Err((id, error)) => {
            if s.last_load_error != Some(id) {
                s.last_load_error = Some(id);
                s.push_control(DaemonMsg::LoadFailed { segment: id, error });
            }
            return;
        }
    };
    // A cursor answered in the session's edition (the list, `Live`) is the
    // source whose list it holds from now; an id's frame moves nothing.
    if cursor_id(cursor).is_none() {
        s.answered_by = Some(i);
    }
    let engine = sources.engine_mut(i);
    let running = engine.game_running;
    // v49: in the edition the session looks in — one entry for the hub and
    // the mock (`Engine::build_cursor`).
    let built = engine.build_cursor(s.kind, cursor, s.edition, running);
    match built {
        Built::Ready(msg) => s.push_snapshot(*msg),
        Built::Loading(msg, id, meta) => {
            request_load(engine, loader, id, meta);
            s.push_snapshot(*msg);
        }
        Built::Failed(id, error) => {
            if s.last_load_error != Some(id) {
                s.last_load_error = Some(id);
                s.push_control(DaemonMsg::LoadFailed { segment: id, error });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoadReq;
    use crate::overlay::Supervisor;
    use crate::session::OUTBOX;
    use crate::sources::HANDOFF_QUIET;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, channel, sync_channel};
    use wowdps_model::Edition;
    use wowdps_proto::SegmentRef;

    /// A session wired to an in-test receiver, the way the server's writer
    /// thread would hold the other end.
    fn session(id: u64) -> (Session, Receiver<DaemonMsg>) {
        let (tx, rx) = sync_channel(OUTBOX);
        (Session::new(id, ClientKind::Tui, 0, tx), rx)
    }

    fn hub_opts() -> HubOptions {
        HubOptions {
            linger: true,
            idle_grace: Duration::from_secs(30),
            tick: Duration::from_millis(20),
            version: "test".to_string(),
            sources: Vec::new(),
        }
    }

    /// A loader channel whose receiver we keep, so `loader.send` succeeds
    /// and we can also assert nothing was requested.
    fn fake_loader() -> (Sender<LoadReq>, Receiver<LoadReq>) {
        channel()
    }

    /// Same minimal damage line as the engine/ipc suites.
    fn hit(min: u32, sec: u32) -> String {
        format!(
            "7/27/2026 21:{min:02}:{sec:02}.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil\n"
        )
    }

    fn seg_cursor(sref: SegmentRef) -> Cursor {
        Cursor::Segment {
            segment: sref,
            view: wowdps_model::View::Damage,
            top_n: None,
            drill: None,
            death: None,
            spell: None,
            range: None,
        }
    }

    /// The load-wakeup filter: only a Segment or Compare cursor pinned to
    /// exactly that id is waiting on its slice. List and Live cursors never
    /// wait — the list needs no parse, and Live is served from the meter.
    #[test]
    fn cursor_wants_matches_only_the_pinned_id() {
        let id = SegmentId(7);
        assert!(!cursor_wants(None, id), "no cursor wants nothing");
        assert!(!cursor_wants(Some(&Cursor::List), id));
        assert!(!cursor_wants(Some(&seg_cursor(SegmentRef::Live)), id));
        assert!(cursor_wants(Some(&seg_cursor(SegmentRef::Id(id))), id));
        assert!(
            !cursor_wants(Some(&seg_cursor(SegmentRef::Id(SegmentId(8)))), id),
            "a different id is a different wait"
        );
        // R12: a comparison waits on its segment exactly like a meter.
        let cmp = Cursor::Compare {
            segment: SegmentRef::Id(id),
            a: "A".to_string(),
            b: "B".to_string(),
            view: wowdps_model::View::Damage,
            range: None,
            spell: None,
        };
        assert!(cursor_wants(Some(&cmp), id));
    }

    /// The 10 Hz tick rebuilds every watched cursor, but an unchanged
    /// snapshot is never pushed — that is what keeps idle clients silent.
    /// When the engine does change, the next push carries a higher seq.
    #[test]
    fn unchanged_snapshots_are_not_repushed_and_seq_stays_monotonic() {
        let mut engine = Sources::new(&[]);
        let (loader, _loader_rx) = fake_loader();
        let (mut s, rx) = session(1);
        s.set_cursor(Cursor::List, wowdps_proto::EditionRef::Follow);

        push_cursor(&mut s, &mut engine, &loader);
        let first = rx.try_recv().expect("the first push always lands");
        let DaemonMsg::SegmentList { seq: seq1, .. } = first else {
            panic!("a List cursor is answered with SegmentList, got {first:?}");
        };

        // Two more ticks with nothing changed: dedup swallows both.
        push_cursor(&mut s, &mut engine, &loader);
        push_cursor(&mut s, &mut engine, &loader);
        assert!(
            rx.try_recv().is_err(),
            "unchanged list must not be repushed"
        );

        // The engine changes shape; the next tick pushes, seq strictly up.
        engine.on_tail(0, TailEvent::Lines(vec![hit(0, 0)]), &mut Vec::new());
        push_cursor(&mut s, &mut engine, &loader);
        let DaemonMsg::SegmentList {
            seq: seq2, entries, ..
        } = rx.try_recv().expect("a changed list is pushed")
        else {
            panic!("expected SegmentList");
        };
        assert!(seq2 > seq1, "per-session seq is monotonic");
        assert_eq!(entries.len(), 1, "the new segment is on it");
    }

    /// v50 (R30): a live Forever pull's roles ride its rows. None under the
    /// solver's 10 s of combat, then its own: a role that arrives is a row
    /// that changed, pushed by the same dedup as any other change, and a
    /// tick that finds nothing new pushes nothing, roles and all.
    #[test]
    fn a_live_role_is_pushed_as_a_row_change_and_not_again_unchanged() {
        const ROLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/roles.txt");
        let text = std::fs::read_to_string(ROLES).unwrap();
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let end = lines
            .iter()
            .position(|l| l.contains("ENCOUNTER_END"))
            .unwrap();
        let mut hub = Hub::new();
        let rx = hub.connect(1, ClientKind::Window);
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/WoWCombatLog-110226_200000.txt")),
        ));
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        drain(&rx);
        // Each pushed snapshot's rows, by how many wear a role.
        let mut pushed = Vec::new();
        for line in &lines[..=end] {
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![line.clone()])));
            hub.tick();
            for m in drain(&rx) {
                if let DaemonMsg::Snapshot { rows, .. } = m
                    && !rows.is_empty()
                {
                    pushed.push(rows.iter().filter(|r| r.role().is_some()).count());
                }
            }
        }
        let first = pushed.iter().position(|&n| n > 0).expect("roles arrive");
        assert!(first > 0, "no role at the pull's first rows: {pushed:?}");
        assert_eq!(pushed.last(), Some(&5), "the five damage rows, settled");
        hub.tick();
        hub.tick();
        assert!(
            !drain(&rx)
                .iter()
                .any(|m| matches!(m, DaemonMsg::Snapshot { .. })),
            "nothing new: nothing pushed"
        );
    }

    /// A `Watch` is answered inside `handle`, not on the next tick: a view
    /// change or drilldown open costs one round trip. Re-declaring the same
    /// cursor resets dedup, so the immediate reply is never suppressed by
    /// the identical snapshot pushed a moment ago.
    #[test]
    fn watch_is_answered_immediately_and_redeclaring_resets_dedup() {
        let mut engine = Sources::new(&[]);
        let (loader, _loader_rx) = fake_loader();
        let mut supervisor = Supervisor::disabled();
        let opts = hub_opts();
        let mut shutdown = false;

        let (s, rx) = session(1);
        let mut sessions = vec![s];

        let watch = |sessions: &mut Vec<Session>,
                     engine: &mut Sources,
                     supervisor: &mut Supervisor,
                     shutdown: &mut bool| {
            handle(
                HubMsg::Client {
                    id: 1,
                    msg: ClientMsg::watch(Cursor::List),
                },
                engine,
                sessions,
                &loader,
                supervisor,
                &opts,
                shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        watch(&mut sessions, &mut engine, &mut supervisor, &mut shutdown);
        assert!(
            matches!(rx.try_recv(), Ok(DaemonMsg::SegmentList { .. })),
            "the reply is already queued when handle returns"
        );

        // Same cursor again: identical content, but set_cursor cleared the
        // dedup slot, so the client still gets its answer.
        watch(&mut sessions, &mut engine, &mut supervisor, &mut shutdown);
        assert!(
            matches!(rx.try_recv(), Ok(DaemonMsg::SegmentList { .. })),
            "re-watching must be answered even when nothing changed"
        );
    }

    /// When the id table changes shape, the fresh list goes to *every*
    /// session — segment watchers and cursorless ones included — because
    /// off-list navigation resolves neighbors by id. A tail batch that only
    /// extends the open segment changes no ids and broadcasts nothing.
    #[test]
    fn id_table_changes_broadcast_the_list_to_every_session() {
        let mut engine = Sources::new(&[]);
        let (loader, _loader_rx) = fake_loader();
        let mut supervisor = Supervisor::disabled();
        let opts = hub_opts();
        let mut shutdown = false;

        let (mut lister, list_rx) = session(1);
        lister.set_cursor(Cursor::List, wowdps_proto::EditionRef::Follow);
        let (mut watcher, watch_rx) = session(2);
        watcher.set_cursor(
            seg_cursor(SegmentRef::Live),
            wowdps_proto::EditionRef::Follow,
        );
        let (idle, idle_rx) = session(3); // connected, no cursor yet
        let mut sessions = vec![lister, watcher, idle];

        let mut tail = |sessions: &mut Vec<Session>, engine: &mut Sources, line: String| {
            handle(
                HubMsg::Tail(0, TailEvent::Lines(vec![line])),
                engine,
                sessions,
                &loader,
                &mut supervisor,
                &opts,
                &mut shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        // First line: a segment appears, the table changes shape.
        tail(&mut sessions, &mut engine, hit(0, 0));
        for (name, rx) in [
            ("lister", &list_rx),
            ("watcher", &watch_rx),
            ("idle", &idle_rx),
        ] {
            assert!(
                matches!(rx.try_recv(), Ok(DaemonMsg::SegmentList { .. })),
                "{name} missed the broadcast"
            );
        }

        // Second line inside the same segment: same ids, no broadcast.
        tail(&mut sessions, &mut engine, hit(0, 1));
        for (name, rx) in [
            ("lister", &list_rx),
            ("watcher", &watch_rx),
            ("idle", &idle_rx),
        ] {
            assert!(
                rx.try_recv().is_err(),
                "{name} got a broadcast though no id changed"
            );
        }
    }

    /// A load failure wakes only the sessions pinned to that id, and only
    /// once — a broken cursor is reported one time, not at 10 Hz.
    #[test]
    fn a_failed_load_notifies_only_its_waiters_and_only_once() {
        let mut engine = Sources::new(&[]);
        let (loader, _loader_rx) = fake_loader();
        let mut supervisor = Supervisor::disabled();
        let opts = hub_opts();
        let mut shutdown = false;

        let wanted = SegmentId(3);
        let (mut a, a_rx) = session(1);
        a.set_cursor(
            seg_cursor(SegmentRef::Id(wanted)),
            wowdps_proto::EditionRef::Follow,
        );
        let (mut b, b_rx) = session(2);
        b.set_cursor(
            seg_cursor(SegmentRef::Id(SegmentId(4))),
            wowdps_proto::EditionRef::Follow,
        );
        let mut sessions = vec![a, b];

        let mut fail = |sessions: &mut Vec<Session>, engine: &mut Sources| {
            handle(
                HubMsg::Loaded {
                    id: wanted,
                    result: Err("disk gone".to_string()),
                },
                engine,
                sessions,
                &loader,
                &mut supervisor,
                &opts,
                &mut shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        fail(&mut sessions, &mut engine);
        match a_rx.try_recv() {
            Ok(DaemonMsg::LoadFailed { segment, error }) => {
                assert_eq!(segment, wanted);
                assert_eq!(error, LoadError::Io("disk gone".to_string()));
            }
            other => panic!("the waiter must hear about its failure: {other:?}"),
        }
        assert!(
            b_rx.try_recv().is_err(),
            "a session watching a different id hears nothing"
        );

        // The loader reporting the same failure again is not news.
        fail(&mut sessions, &mut engine);
        assert!(
            a_rx.try_recv().is_err(),
            "the same failure must not be re-reported"
        );
    }

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");

    /// Every argument `handle` takes, bundled so a test reads as messages.
    struct Hub {
        sources: Sources,
        sessions: Vec<Session>,
        loader: Sender<LoadReq>,
        loader_rx: Receiver<LoadReq>,
        supervisor: Supervisor,
        opts: HubOptions,
        shutdown: bool,
        history: HistoryLink,
    }

    impl Hub {
        fn new() -> Self {
            Self::with_sources(Vec::new())
        }

        /// v49: a hub following one source per setup, in order.
        fn with_sources(setups: Vec<SourceSetup>) -> Self {
            let (loader, loader_rx) = fake_loader();
            let mut opts = hub_opts();
            opts.sources = setups;
            Hub {
                sources: Sources::new(&opts.sources),
                sessions: Vec::new(),
                loader,
                loader_rx,
                supervisor: Supervisor::new(false, Duration::ZERO, None),
                opts,
                shutdown: false,
                history: HistoryLink::disabled("test"),
            }
        }

        /// The first source's engine: a daemon of one source's only one.
        fn engine(&self) -> &Engine {
            self.sources.primary()
        }

        /// A tick: every session's cursor rebuilt and pushed when changed.
        fn tick(&mut self) {
            tick(&mut self.sources, &mut self.sessions, &self.loader);
        }

        fn connect(&mut self, id: u64, kind: ClientKind) -> Receiver<DaemonMsg> {
            let (tx, rx) = sync_channel(OUTBOX);
            self.handle(HubMsg::Connected {
                id,
                kind,
                pid: 0,
                tx,
            });
            assert!(
                matches!(rx.try_recv(), Ok(DaemonMsg::HelloAck { .. })),
                "every connection is acked"
            );
            rx
        }

        fn handle(&mut self, msg: HubMsg) {
            handle(
                msg,
                &mut self.sources,
                &mut self.sessions,
                &self.loader,
                &mut self.supervisor,
                &self.opts,
                &mut self.shutdown,
                &self.history,
            );
        }

        fn client(&mut self, id: u64, msg: ClientMsg) {
            self.handle(HubMsg::Client { id, msg });
        }

        /// The fixture as the tail thread would deliver it: source, index,
        /// caught up — history served lazily.
        fn index_fixture(&mut self) -> Vec<SegmentId> {
            let mut file = std::fs::File::open(FIXTURE).unwrap();
            let idx = wowdps_core::index::scan(&mut file);
            self.handle(HubMsg::Tail(0, TailEvent::Switched(FIXTURE.into())));
            self.handle(HubMsg::Tail(
                0,
                TailEvent::Index {
                    index: Box::new(idx),
                    file_age_ms: Some(0),
                },
            ));
            self.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            self.engine().list_ids()
        }
    }

    fn drain(rx: &Receiver<DaemonMsg>) -> Vec<DaemonMsg> {
        let mut out = Vec::new();
        while let Ok(m) = rx.try_recv() {
            out.push(m);
        }
        out
    }

    /// v49: the list's change is its ids OR its edition: a log whose first
    /// pull closed before any version line, then a line naming Forever —
    /// no id moves, the source's edition does. The list is broadcast again:
    /// a session pinned to Forever (watching `Live`, so the broadcast is
    /// its only id table) is sent it now, and a follower's names Forever.
    #[test]
    fn the_list_is_broadcast_again_when_only_its_edition_changes() {
        use wowdps_model::Edition;
        use wowdps_proto::EditionRef;
        let mut hub = Hub::new();
        let follower = hub.connect(1, ClientKind::Tui);
        let pinned = hub.connect(2, ClientKind::Tui);
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Live),
                edition: EditionRef::Pin(Edition::Forever),
            },
        );
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/a.txt")),
        ));
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Lines(vec![
                "7/27/2026 21:00:10.000-4  ENCOUNTER_START,3130,\"Boss\",1,5,2769".to_string(),
                hit(0, 11),
                "7/27/2026 21:00:40.000-4  ENCOUNTER_END,3130,\"Boss\",1,5,1,30000".to_string(),
            ]),
        ));
        let ids = hub.engine().list_ids();
        assert!(!ids.is_empty());
        assert_eq!(hub.engine().edition(), None);
        drain(&follower);
        drain(&pinned);
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![
            "7/27/2026 21:05:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18".to_string(),
        ])));
        assert_eq!(hub.engine().edition(), Some(Edition::Forever));
        assert_eq!(hub.engine().list_ids(), ids, "no id moved");

        let last_list = |msgs: Vec<DaemonMsg>| {
            msgs.into_iter().rev().find_map(|m| match m {
                DaemonMsg::SegmentList {
                    entries, edition, ..
                } => Some((entries.len(), edition)),
                _ => None,
            })
        };
        assert_eq!(
            last_list(drain(&pinned)),
            Some((ids.len(), Some(Edition::Forever))),
            "the pinned session's list"
        );
        assert_eq!(
            last_list(drain(&follower)),
            Some((ids.len(), Some(Edition::Forever))),
            "the follower's list names the edition"
        );
    }

    /// A log as the tail delivers it: `Switched`, `Index`, then its seed
    /// lines; what the hub had sent each receiver drained after each.
    fn deliver_log(hub: &mut Hub, path: &str) -> usize {
        deliver_log_to(hub, 0, path, 0)
    }

    /// A log as the tail delivers it to the first source, in its three
    /// messages: `Switched`, `Index` (written just now), `Seeds`.
    fn log_parts(path: &str) -> (HubMsg, HubMsg, HubMsg) {
        let bytes = std::fs::read(path).unwrap();
        let idx = wowdps_core::index::scan(&mut &bytes[..]);
        let seed_ranges = match idx.open.as_ref() {
            Some(open) => open.seeds.clone(),
            None => idx.checkpoint.seeds.clone(),
        };
        let seeds: Vec<String> = seed_ranges
            .iter()
            .map(|&(a, b)| {
                String::from_utf8_lossy(&bytes[a as usize..b as usize])
                    .trim_end()
                    .to_string()
            })
            .collect();
        (
            HubMsg::Tail(0, TailEvent::Switched(path.into())),
            HubMsg::Tail(
                0,
                TailEvent::Index {
                    index: Box::new(idx),
                    file_age_ms: Some(0),
                },
            ),
            HubMsg::Tail(0, TailEvent::Seeds(seeds)),
        )
    }

    /// v49: [`deliver_log`] to source `i`, the log last written `age_ms`
    /// ago.
    fn deliver_log_to(hub: &mut Hub, i: usize, path: &str, age_ms: u64) -> usize {
        let bytes = std::fs::read(path).unwrap();
        let idx = wowdps_core::index::scan(&mut &bytes[..]);
        let seed_ranges = match idx.open.as_ref() {
            Some(open) => open.seeds.clone(),
            None => idx.checkpoint.seeds.clone(),
        };
        let seeds: Vec<String> = seed_ranges
            .iter()
            .map(|&(a, b)| {
                String::from_utf8_lossy(&bytes[a as usize..b as usize])
                    .trim_end()
                    .to_string()
            })
            .collect();
        hub.handle(HubMsg::Tail(i, TailEvent::Switched(path.into())));
        hub.handle(HubMsg::Tail(
            i,
            TailEvent::Index {
                index: Box::new(idx),
                file_age_ms: Some(age_ms),
            },
        ));
        let rows = hub.sources.engine(i).list_ids().len();
        hub.handle(HubMsg::Tail(i, TailEvent::Seeds(seeds)));
        rows
    }

    /// v49: a session pinned to Retail, watching `Live`, on a rotation from
    /// a Retail log to a Forever one, delivered as the tail delivers it: it
    /// is never sent the Forever log, not even its name — between the
    /// switch and the index nothing names the new log's edition, so a pin
    /// is not served it, and the index names it Forever — only Retail's
    /// empty list, which names no log.
    #[test]
    fn a_pin_is_never_listed_another_editions_log_while_its_seeds_are_on_the_way() {
        use wowdps_model::Edition;
        use wowdps_proto::EditionRef;
        const FOREVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");
        let mut hub = Hub::new();
        let retail = hub.connect(1, ClientKind::Tui);
        hub.client(
            1,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Live),
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        assert!(deliver_log(&mut hub, FIXTURE) > 0);
        let lists = |msgs: Vec<DaemonMsg>| -> Vec<(usize, Option<Edition>, Option<String>)> {
            msgs.into_iter()
                .filter_map(|m| match m {
                    DaemonMsg::SegmentList {
                        entries,
                        edition,
                        source,
                        ..
                    } => Some((entries.len(), edition, source)),
                    _ => None,
                })
                .collect()
        };
        let before = lists(drain(&retail));
        assert!(
            before.last().is_some_and(|l| l.0 > 0 && l.2.is_some()),
            "served the Retail log: {before:?}"
        );

        let rows = deliver_log(&mut hub, FOREVER);
        assert!(rows > 0, "the Forever index has rows to leak");
        assert_eq!(hub.engine().edition(), Some(Edition::Forever));
        let after = lists(drain(&retail));
        assert!(!after.is_empty());
        assert!(
            after.iter().all(|l| *l == (0, Some(Edition::Retail), None)),
            "only Retail's empty list, naming no log: {after:?}"
        );
    }

    /// v49: the index landing ends a pin's wait, even an empty index of a
    /// log with no header yet (whose list key does not move): a Retail pin
    /// is then served that log (naming none, it is reckoned Retail's) and
    /// sent its list; a follower is sent nothing it was not sent before.
    #[test]
    fn an_empty_index_ends_a_pins_wait() {
        use wowdps_model::Edition;
        use wowdps_proto::EditionRef;
        let mut hub = Hub::new();
        let follower = hub.connect(1, ClientKind::Tui);
        let retail = hub.connect(2, ClientKind::Tui);
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Live),
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        drain(&follower);
        drain(&retail);
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/WoWCombatLog-new.txt")),
        ));
        assert!(!hub.engine().serves(EditionRef::Pin(Edition::Retail)));
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Index {
                index: Box::default(),
                file_age_ms: Some(0),
            },
        ));
        assert!(hub.engine().serves(EditionRef::Pin(Edition::Retail)));
        let got = drain(&retail);
        assert!(
            got.iter().any(|m| matches!(
                m,
                DaemonMsg::SegmentList {
                    source: Some(s),
                    edition: Some(Edition::Retail),
                    ..
                } if s == "WoWCombatLog-new.txt"
            )),
            "{got:?}"
        );
        assert!(
            drain(&follower).is_empty(),
            "the follower's list did not change"
        );
    }

    /// v49: a session that STOPS being served — a log whose first fight came
    /// before any version line (reckoned Retail's), then a line naming
    /// Forever — is sent its pinned edition's empty list at once, never left
    /// holding the other edition's ids.
    #[test]
    fn a_session_that_stops_being_served_is_sent_its_empty_list() {
        use wowdps_model::Edition;
        use wowdps_proto::EditionRef;
        let mut hub = Hub::new();
        let retail = hub.connect(1, ClientKind::Tui);
        hub.client(
            1,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Live),
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        drain(&retail);
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/a.txt")),
        ));
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![hit(0, 1)])));
        assert_eq!(hub.engine().edition(), None);
        let list = |msgs: Vec<DaemonMsg>| {
            msgs.into_iter().rev().find_map(|m| match m {
                DaemonMsg::SegmentList {
                    entries, edition, ..
                } => Some((entries.len(), edition)),
                _ => None,
            })
        };
        assert_eq!(
            list(drain(&retail)),
            Some((1, Some(Edition::Retail))),
            "a log naming none serves a Retail pin"
        );
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![
            "7/27/2026 21:05:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18".to_string(),
        ])));
        assert_eq!(hub.engine().edition(), Some(Edition::Forever));
        assert_eq!(
            list(drain(&retail)),
            Some((0, Some(Edition::Retail))),
            "emptied when it stops being served"
        );
    }

    /// v49: one source, three sessions — following, pinned to the edition
    /// the log names, pinned to another. The first two are answered alike,
    /// every frame naming the log's edition; the third is answered the
    /// empty list of its own edition and hears nothing of this log: no
    /// fight opening, never its list (a broadcast sends it its own empty
    /// one), no `Live` loadout.
    #[test]
    fn a_session_pinned_to_another_edition_hears_nothing_of_this_log() {
        use wowdps_model::Edition;
        use wowdps_proto::EditionRef;
        let mut hub = Hub::new();
        let follower = hub.connect(1, ClientKind::Tui);
        let retail = hub.connect(2, ClientKind::Tui);
        let forever = hub.connect(3, ClientKind::Tui);
        hub.client(1, ClientMsg::watch(Cursor::List));
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        hub.client(
            3,
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition: EditionRef::Pin(Edition::Forever),
            },
        );
        let list_edition = |msgs: &[DaemonMsg]| -> Vec<(usize, Option<Edition>)> {
            msgs.iter()
                .filter_map(|m| match m {
                    DaemonMsg::SegmentList {
                        entries, edition, ..
                    } => Some((entries.len(), *edition)),
                    _ => None,
                })
                .collect()
        };
        // Before any log: nothing named, and a pin to an edition no source
        // has is still answered — empty, in its own edition.
        assert_eq!(list_edition(&drain(&follower)), [(0, None)]);
        assert_eq!(
            list_edition(&drain(&retail)),
            [(0, Some(Edition::Retail))],
            "a log yet to name one is reckoned Retail's"
        );
        assert_eq!(
            list_edition(&drain(&forever)),
            [(0, Some(Edition::Forever))]
        );

        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/a.txt")),
        ));
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![
            "7/27/2026 21:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1".to_string(),
            hit(0, 1),
        ])));
        assert_eq!(hub.engine().edition(), Some(Edition::Retail));
        for rx in [&follower, &retail] {
            let got = drain(rx);
            assert!(
                got.iter()
                    .any(|m| matches!(m, DaemonMsg::SegmentOpened { .. })),
                "{got:?}"
            );
            assert_eq!(list_edition(&got), [(1, Some(Edition::Retail))]);
        }
        let got = drain(&forever);
        assert!(
            !got.iter()
                .any(|m| matches!(m, DaemonMsg::SegmentOpened { .. })),
            "another edition's fight: {got:?}"
        );
        assert!(
            list_edition(&got)
                .iter()
                .all(|l| *l == (0, Some(Edition::Forever))),
            "only its own empty list: {got:?}"
        );

        // Its Watch is answered at once, and `Live` there is nobody's.
        hub.client(
            3,
            ClientMsg::Watch {
                cursor: Cursor::Segment {
                    segment: SegmentRef::Live,
                    view: wowdps_model::View::Damage,
                    top_n: None,
                    drill: None,
                    death: None,
                    spell: None,
                    range: None,
                },
                edition: EditionRef::Pin(Edition::Forever),
            },
        );
        hub.client(
            3,
            ClientMsg::GetLoadout {
                req_id: 5,
                segment: SegmentRef::Live,
                guid: "Player-1-A".to_string(),
            },
        );
        match drain(&forever).as_slice() {
            [
                DaemonMsg::Snapshot {
                    id: None,
                    rows,
                    info,
                    source: None,
                    edition: Some(Edition::Forever),
                    ..
                },
                DaemonMsg::Loadout {
                    req_id: 5,
                    loadout: None,
                    ..
                },
            ] if rows.is_empty() && info.edition == Edition::Forever => {}
            other => panic!("an empty answer in Forever: {other:?}"),
        }

        // Status lists the one source, live, named by its log.
        hub.client(1, ClientMsg::GetStatus { req_id: 6 });
        match drain(&follower).as_slice() {
            [DaemonMsg::Status { editions, .. }] => {
                assert_eq!(editions.len(), 1);
                assert_eq!(editions[0].edition, Some(Edition::Retail));
                assert_eq!(editions[0].log.as_deref(), Some("a.txt"));
                assert!(editions[0].live);
            }
            other => panic!("{other:?}"),
        }
    }

    /// A switch to a newer log retires the one left behind to the history
    /// thread — and only a real switch: the first log of the daemon's life
    /// and a re-announce of the same file retire nothing.
    #[test]
    fn switching_logs_retires_the_previous_one_to_history() {
        let mut hub = Hub::new();
        let (link, rx) = HistoryLink::bounded(8);
        hub.history = link;
        let retired = |rx: &Receiver<HistoryReq>| -> Vec<PathBuf> {
            let mut out = Vec::new();
            while let Ok(req) = rx.try_recv() {
                if let HistoryReq::Retire(p) = req {
                    out.push(p);
                }
            }
            out
        };

        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/a.txt")),
        ));
        assert!(retired(&rx).is_empty(), "nothing precedes the first log");
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/a.txt")),
        ));
        assert!(retired(&rx).is_empty(), "the same file is not a switch");
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/logs/b.txt")),
        ));
        assert_eq!(retired(&rx), vec![PathBuf::from("/logs/a.txt")]);
        assert_eq!(
            hub.engine().source_path.as_deref(),
            Some(Path::new("/logs/b.txt"))
        );
    }

    /// The game watcher's verdict reaches the supervisor, whose commands go
    /// to the overlay session only; the overlay's own visibility report
    /// shapes what `Status` says.
    #[test]
    fn game_transitions_drive_the_overlay_session_and_status() {
        use wowdps_proto::OverlayState;
        let mut hub = Hub::new();
        let tui = hub.connect(1, ClientKind::Tui);
        let overlay = hub.connect(2, ClientKind::Overlay);

        hub.handle(HubMsg::Game(Running::unnamed()));
        assert!(hub.sources.running().any());
        assert_eq!(drain(&overlay), vec![DaemonMsg::SetVisible(true)]);
        assert!(drain(&tui).is_empty(), "a meter client hears nothing");

        hub.client(2, ClientMsg::VisibilityChanged { visible: false });
        hub.client(1, ClientMsg::GetStatus { req_id: 4 });
        match drain(&tui).as_slice() {
            [
                DaemonMsg::Status {
                    req_id: 4,
                    game_running: true,
                    clients: 2,
                    overlay: OverlayState::Hidden,
                    ..
                },
            ] => {}
            other => panic!("status must reflect the game and the manual hide: {other:?}"),
        }

        hub.handle(HubMsg::Game(Running::default()));
        assert_eq!(drain(&overlay), vec![DaemonMsg::SetVisible(false)]);
        hub.handle(HubMsg::Game(Running::default()));
        assert!(drain(&overlay).is_empty(), "no transition, no command");

        // Hello and Shutdown: the former is a no-op here, the latter stops.
        hub.client(
            1,
            ClientMsg::Hello {
                proto: PROTO_VERSION,
                client: ClientKind::Tui,
                pid: 1,
            },
        );
        assert!(!hub.shutdown);
        hub.client(1, ClientMsg::Shutdown);
        assert!(hub.shutdown);
        // A message from a session that never connected is dropped.
        hub.client(99, ClientMsg::GetStatus { req_id: 1 });
        assert!(drain(&tui).is_empty());
    }

    /// Every way out of `run` takes down the overlay the daemon spawned: a
    /// `Shutdown`, and every sender hanging up. Left running, the orphan's
    /// reconnect respawns a daemon, which spawns another overlay.
    #[test]
    fn leaving_run_terminates_the_spawned_overlay() {
        use std::sync::atomic::Ordering;
        for shutdown in [true, false] {
            let (supervisor, probes) =
                crate::overlay::tests::supervisor(Duration::from_secs(3600), None);
            let (tx, rx) = channel();
            let (loader, _loader_rx) = fake_loader();
            let (done_tx, done_rx) = channel();
            std::thread::spawn(move || {
                run(
                    rx,
                    loader,
                    supervisor,
                    hub_opts(),
                    HistoryLink::disabled("test"),
                );
                let _ = done_tx.send(());
            });
            tx.send(HubMsg::Game(Running::unnamed())).unwrap();
            if shutdown {
                tx.send(HubMsg::Client {
                    id: 1,
                    msg: ClientMsg::Shutdown,
                })
                .unwrap();
            } else {
                drop(tx);
            }
            done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("run returned");
            assert_eq!(
                probes.spawns.load(Ordering::SeqCst),
                1,
                "the game spawned it"
            );
            assert_eq!(
                probes.terminated.load(Ordering::SeqCst),
                1,
                "shutdown {shutdown}: the spawned overlay outlived the daemon"
            );
        }
    }

    /// v19: a warm segment answers `GetLoadout` inline; a cold one parks the
    /// request behind exactly one load and answers when the slice lands —
    /// with the build on success, with `None` on a failed load.
    #[test]
    fn get_loadout_is_answered_warm_and_parked_cold() {
        let mut hub = Hub::new();
        let rx = hub.connect(1, ClientKind::Window);
        let ids = hub.index_fixture();
        // The boss pulls: COMBATANT_INFO fires at ENCOUNTER_START, so their
        // slices know the build (the opening trash does not).
        let bosses: Vec<SegmentId> = match hub.engine().build_list(false) {
            DaemonMsg::SegmentList { entries, .. } => entries
                .iter()
                .filter(|e| e.row.kind == wowdps_model::SegmentKind::Encounter)
                .map(|e| e.id)
                .collect(),
            other => panic!("{other:?}"),
        };
        assert_eq!(bosses.len(), 2);
        assert!(ids.len() > 2);
        assert!(
            drain(&rx)
                .iter()
                .any(|m| matches!(m, DaemonMsg::SegmentList { .. }))
        );
        let first = bosses[0];
        let guid = "Player-1168-0A1B2C01".to_string();

        // Nothing live: a Live query is answered at once with None.
        hub.client(
            1,
            ClientMsg::GetLoadout {
                req_id: 1,
                segment: SegmentRef::Id(SegmentId(u64::MAX)),
                guid: guid.clone(),
            },
        );
        assert!(matches!(
            drain(&rx).as_slice(),
            [DaemonMsg::Loadout {
                req_id: 1,
                loadout: None,
                ..
            }]
        ));

        // Cold history: parked, one load requested.
        for req_id in [2, 3] {
            hub.client(
                1,
                ClientMsg::GetLoadout {
                    req_id,
                    segment: SegmentRef::Id(first),
                    guid: guid.clone(),
                },
            );
        }
        assert!(drain(&rx).is_empty(), "parked, not answered");
        let req = hub.loader_rx.try_recv().expect("one load requested");
        assert_eq!(req.id, first);
        assert!(
            hub.loader_rx.try_recv().is_err(),
            "the second request rides the same load"
        );

        let text = wowdps_core::index::load_segment_text(&req.path, &req.meta).unwrap();
        let meter = text.meter();
        hub.handle(HubMsg::Loaded {
            id: first,
            result: Ok(Box::new(meter)),
        });
        let answers: Vec<u32> = drain(&rx)
            .into_iter()
            .map(|m| match m {
                DaemonMsg::Loadout {
                    req_id,
                    loadout: Some(l),
                    ..
                } => {
                    assert_eq!(l.spec_id, Some(71));
                    req_id
                }
                other => panic!("expected loaded answers, got {other:?}"),
            })
            .collect();
        assert_eq!(answers, [2, 3], "both parked requests answered in order");

        // A failed load still answers every parked one-shot — with None.
        let second = bosses[1];
        hub.client(
            1,
            ClientMsg::GetLoadout {
                req_id: 4,
                segment: SegmentRef::Id(second),
                guid: guid.clone(),
            },
        );
        hub.handle(HubMsg::Loaded {
            id: second,
            result: Err("disk gone".to_string()),
        });
        assert!(matches!(
            drain(&rx).as_slice(),
            [DaemonMsg::Loadout {
                req_id: 4,
                loadout: None,
                ..
            }]
        ));
    }

    /// R11: the trash can shrinks the id table, and a shrunk table is
    /// broadcast exactly like a grown one.
    #[test]
    fn discard_trash_broadcasts_the_shorter_list() {
        let mut hub = Hub::new();
        let rx = hub.connect(1, ClientKind::Tui);
        // Two hits a trash-gap apart: one closed trash, one live segment.
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Lines(vec![hit(0, 0), hit(10, 0)]),
        ));
        let before = drain(&rx);
        assert!(matches!(
            before.last(),
            Some(DaemonMsg::SegmentList { entries, .. }) if entries.len() == 2
        ));

        hub.client(1, ClientMsg::DiscardTrash);
        match drain(&rx).as_slice() {
            [DaemonMsg::SegmentList { entries, .. }] => {
                assert_eq!(entries.len(), 1);
                assert!(entries[0].row.live);
            }
            other => panic!("the shorter list must be broadcast: {other:?}"),
        }
        hub.client(1, ClientMsg::DiscardTrash);
        assert!(
            drain(&rx).is_empty(),
            "nothing more to discard, nothing sent"
        );
    }

    /// A comparison cursor is served through the same tick path as a
    /// meter, and disconnecting removes the session.
    #[test]
    fn compare_cursors_are_pushed_and_disconnects_reap() {
        let mut hub = Hub::new();
        let rx = hub.connect(1, ClientKind::Window);
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![hit(0, 0)])));
        drain(&rx);
        hub.client(
            1,
            ClientMsg::watch(Cursor::Compare {
                segment: SegmentRef::Live,
                a: "Player-1-A".to_string(),
                b: "Player-1-B".to_string(),
                view: wowdps_model::View::Damage,
                range: None,
                spell: None,
            }),
        );
        assert!(matches!(
            drain(&rx).as_slice(),
            [DaemonMsg::CompareSnapshot { a, .. }] if a.total.amount == 900
        ));
        hub.handle(HubMsg::Disconnected { id: 1 });
        assert!(hub.sessions.is_empty());
    }

    /// v35 (R25): the raid timeline is built for the clients that use it —
    /// the window draws it, the mcp answers it — and never for the overlay
    /// or the TUI, whose 10 Hz pushes would build and encode the group's
    /// whole series for nothing.
    #[test]
    fn the_raid_timeline_goes_only_to_the_clients_that_use_it() {
        let mut hub = Hub::new();
        let kinds = [
            (1, ClientKind::Window, true),
            (2, ClientKind::Mcp, true),
            (3, ClientKind::Overlay, false),
            (4, ClientKind::Tui, false),
        ];
        let rxs: Vec<_> = kinds
            .iter()
            .map(|(id, kind, want)| (hub.connect(*id, *kind), *kind, *want))
            .collect();
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(vec![hit(0, 0)])));
        for (rx, _, _) in &rxs {
            drain(rx);
        }
        for (id, _, _) in kinds {
            hub.client(
                id,
                ClientMsg::watch(Cursor::Segment {
                    segment: SegmentRef::Live,
                    view: wowdps_model::View::Damage,
                    top_n: None,
                    drill: None,
                    death: None,
                    spell: None,
                    range: None,
                }),
            );
        }
        for (rx, kind, want) in &rxs {
            let raid = drain(rx).into_iter().find_map(|m| match m {
                DaemonMsg::Snapshot { raid, .. } => Some(raid.is_some()),
                _ => None,
            });
            assert_eq!(raid, Some(*want), "{kind:?}");
        }
    }

    // ---- v49: one source and one engine per edition ----------------------

    const FOREVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/forever.txt");

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    /// A boss pull opening on Retail's clock after `sample.txt`'s end, at
    /// `min` minutes past 20:00.
    fn retail_pull(min: u32) -> Vec<String> {
        vec![
            format!(
                "7/27/2026 20:{min:02}:00.000-4  ENCOUNTER_START,3130,\"The Ashen Warden\",15,3,2769"
            ),
            format!(
                "7/27/2026 20:{min:02}:01.000-4  SPELL_DAMAGE,Player-1168-0A1B2C01,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil"
            ),
        ]
    }

    fn retail_end(min: u32) -> Vec<String> {
        vec![format!(
            "7/27/2026 20:{min:02}:30.000-4  ENCOUNTER_END,3130,\"The Ashen Warden\",15,3,1,30000"
        )]
    }

    /// The same pull at `min` minutes past 10:00 on 11/2/2026, UTC−8: on
    /// the clock both logs' timed lines share in the live edition's tests
    /// ([`stamp`]), where lines must be later than the live log's to take it.
    fn retail_pull_at(min: u32) -> Vec<String> {
        retail_pull(min)
            .iter()
            .map(|l| {
                l.replacen("7/27/2026 20:", "11/2/2026 10:", 1)
                    .replacen("-4 ", "-8 ", 1)
            })
            .collect()
    }

    fn retail_end_at(min: u32) -> Vec<String> {
        retail_end(min)
            .iter()
            .map(|l| {
                l.replacen("7/27/2026 20:", "11/2/2026 10:", 1)
                    .replacen("-4 ", "-8 ", 1)
            })
            .collect()
    }

    /// A boss pull opening in Forever's log at `min` minutes past 10:00 on
    /// 11/2/2026, UTC−8 ([`retail_pull_at`]'s clock), after `forever.txt`'s
    /// end.
    fn forever_pull(min: u32) -> Vec<String> {
        vec![
            format!("11/2/2026 10:{min:02}:00.000-8  ENCOUNTER_START,2914,\"Geilhast\",1,5,48"),
            format!(
                "11/2/2026 10:{min:02}:01.000-8  SPELL_DAMAGE,Player-6101-0A2B3C01,\"Velkris-Duskmantle-Nebula-US\",0x511,0x80000000,Creature-0-6101-48-2001-990001-0000A0B001,\"Geilhast\",0xa48,0x80000000,1752,\"Sinister Strike\",0x1,Creature-0-6101-48-2001-990001-0000A0B001,0000000000000000,1170,1290,93,0,932,0,0,0,1,0,0,0,-507.79,-11.67,0,0.3814,22,120,125,-1,1,0,0,0,nil,nil,nil,ST"
            ),
        ]
    }

    fn forever_end(min: u32) -> Vec<String> {
        vec![format!(
            "11/2/2026 10:{min:02}:31.000-8  ENCOUNTER_END,2914,\"Geilhast\",1,5,1,21000"
        )]
    }

    /// Retail's folder and Forever's, each named by its product folder.
    fn two_editions() -> Hub {
        Hub::with_sources(vec![
            SourceSetup {
                spec: Some("logs:/wow/_retail_/Logs".to_string()),
                edition: Some(Edition::Retail),
            },
            SourceSetup {
                spec: Some("logs:/wow/_forever_/Logs".to_string()),
                edition: Some(Edition::Forever),
            },
        ])
    }

    /// Both logs delivered and caught up: Retail's written an hour ago,
    /// Forever's just now; then a tick, which chooses the live edition.
    fn deliver_both(hub: &mut Hub) {
        deliver_log_to(hub, 0, FIXTURE, 3_600_000);
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        deliver_log_to(hub, 1, FOREVER, 0);
        hub.handle(HubMsg::Tail(1, TailEvent::CaughtUp));
        hub.tick();
    }

    /// The last list in `msgs`: its rows' editions, the edition it names and
    /// the log it names.
    fn last_list(msgs: &[DaemonMsg]) -> Option<(Vec<Edition>, Option<Edition>, Option<String>)> {
        msgs.iter().rev().find_map(|m| match m {
            DaemonMsg::SegmentList {
                entries,
                edition,
                source,
                ..
            } => Some((
                entries.iter().map(|e| e.row.edition).collect(),
                *edition,
                source.clone(),
            )),
            _ => None,
        })
    }

    fn opened(msgs: &[DaemonMsg]) -> Vec<SegmentId> {
        msgs.iter()
            .filter_map(|m| match m {
                DaemonMsg::SegmentOpened { id } => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// A frame a scenario recorded: the step, the session and the message.
    struct Frame {
        step: &'static str,
        session: u64,
        msg: DaemonMsg,
    }

    impl Frame {
        /// The frame as a reader sees it, for a failing assertion.
        fn line(&self) -> String {
            format!("{} s{}: {:?}", self.step, self.session, self.msg)
        }

        /// The golden's line: step, session, message kind, and FNV-1a of
        /// the frame's wire bytes (`DaemonMsg::encode`: length, tag, body)
        /// — what the session's socket carries, not how it prints.
        fn golden(&self) -> String {
            let text = format!("{:?}", self.msg);
            let kind: String = text.chars().take_while(|c| c.is_alphanumeric()).collect();
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in self.msg.encode() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            format!("{} s{} {kind} {h:016x}", self.step, self.session)
        }
    }

    /// Every frame of a scenario that exercises everything a session can
    /// be answered — following and pinned, the list, `Live`, an id cold and
    /// loaded, one-shots, the trash can, a comparison, rotated and unknown
    /// ids, the game, and a rotation to another edition's log — per step
    /// and session, in order.
    fn one_source_scenario(mut hub: Hub) -> Vec<Frame> {
        let mut dump = Vec::new();
        let kinds = [
            (
                1,
                ClientKind::Window,
                EditionRef::Follow,
                seg_cursor(SegmentRef::Live),
            ),
            (
                2,
                ClientKind::Tui,
                EditionRef::Pin(Edition::Retail),
                Cursor::List,
            ),
            (
                3,
                ClientKind::Tui,
                EditionRef::Pin(Edition::Forever),
                seg_cursor(SegmentRef::Live),
            ),
            (4, ClientKind::Mcp, EditionRef::Follow, Cursor::List),
            (
                5,
                ClientKind::Overlay,
                EditionRef::Follow,
                seg_cursor(SegmentRef::Live),
            ),
        ];
        let rxs: Vec<(u64, Receiver<DaemonMsg>)> = kinds
            .iter()
            .map(|(id, kind, _, _)| (*id, hub.connect(*id, *kind)))
            .collect();
        let record = |step: &'static str, dump: &mut Vec<Frame>| {
            for (id, rx) in &rxs {
                for msg in drain(rx) {
                    dump.push(Frame {
                        step,
                        session: *id,
                        msg,
                    });
                }
            }
        };
        for (id, _, edition, cursor) in &kinds {
            hub.client(
                *id,
                ClientMsg::Watch {
                    cursor: cursor.clone(),
                    edition: *edition,
                },
            );
        }
        record("watch", &mut dump);
        hub.tick();
        record("tick", &mut dump);
        hub.handle(HubMsg::Game(Running::of(&[Edition::Retail])));
        record("game", &mut dump);
        let (switch, index, seeds) = log_parts(FIXTURE);
        hub.handle(switch);
        record("deliver switched", &mut dump);
        hub.handle(index);
        hub.handle(seeds);
        record("deliver", &mut dump);
        hub.tick();
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        hub.tick();
        record("caught up", &mut dump);
        let boss = hub
            .engine()
            .list_ids()
            .into_iter()
            .zip(hub.engine().list_rows())
            .find(|(_, r)| r.kind == wowdps_model::SegmentKind::Encounter)
            .map(|(id, _)| id)
            .unwrap();
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Id(boss))));
        let req = hub.loader_rx.try_recv().expect("a load");
        let text = wowdps_core::index::load_segment_text(&req.path, &req.meta).unwrap();
        hub.handle(HubMsg::Loaded {
            id: req.id,
            result: Ok(Box::new(text.meter())),
        });
        record("loaded", &mut dump);
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_pull(20))));
        hub.tick();
        record("pull", &mut dump);
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_end(20))));
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(lines(&[
            "7/27/2026 20:22:00.000-4  SPELL_DAMAGE,Player-1168-0A1B2C01,\"Ana-Realm\",0x511,0x0,Creature-0-8,\"Trash\",0xa48,0x0,116,\"Frostbolt\",16,300,300,0,0,0,0,0,nil,nil",
        ]))));
        hub.tick();
        record("trash", &mut dump);
        hub.client(4, ClientMsg::GetStatus { req_id: 7 });
        for (session, req_id) in [(1, 8), (3, 9)] {
            hub.client(
                session,
                ClientMsg::GetLoadout {
                    req_id,
                    segment: SegmentRef::Live,
                    guid: "Player-1168-0A1B2C01".to_string(),
                },
            );
        }
        hub.client(4, ClientMsg::DiscardTrash);
        hub.tick();
        record("one-shots", &mut dump);
        hub.client(
            1,
            ClientMsg::watch(Cursor::Compare {
                segment: SegmentRef::Live,
                a: "Player-1168-0A1B2C01".to_string(),
                b: "Player-1168-0A1B2C02".to_string(),
                view: wowdps_model::View::Damage,
                range: None,
                spell: None,
            }),
        );
        hub.client(
            5,
            ClientMsg::watch(seg_cursor(SegmentRef::Id(SegmentId(999_999)))),
        );
        hub.client(
            4,
            ClientMsg::watch(seg_cursor(SegmentRef::Id(SegmentId(0)))),
        );
        hub.tick();
        record("ids", &mut dump);
        hub.handle(HubMsg::Game(Running::default()));
        hub.tick();
        record("no game", &mut dump);
        let (switch, index, seeds) = log_parts(FOREVER);
        hub.handle(switch);
        record("rotate switched", &mut dump);
        hub.handle(index);
        hub.handle(seeds);
        record("rotate", &mut dump);
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        hub.tick();
        hub.client(4, ClientMsg::watch(seg_cursor(SegmentRef::Id(boss))));
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Live),
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        hub.client(
            3,
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition: EditionRef::Pin(Edition::Forever),
            },
        );
        hub.tick();
        hub.client(4, ClientMsg::GetStatus { req_id: 10 });
        record("end", &mut dump);
        dump
    }

    /// How a daemon of one source words its first source in `Status`.
    const RETAIL_SPEC: &str = "logs:/wow/_retail_/Logs";

    /// The one-source scenario's frames as the daemon sent them before it
    /// followed more than one source: produced by `0b5714f`'s hub over the
    /// same scenario (`one_source_scenario`, ported to that hub's API), a
    /// line per frame — step, session, kind, and FNV-1a of its wire bytes.
    const ONE_SOURCE_GOLDEN: &str = include_str!("../tests/golden/one_source_frames.txt");

    /// The Retail guard: a daemon of one source — `--file`, `--logs`, a
    /// folder outside an install — queues every session of the scenario the
    /// same frames, in the same order, byte for byte on the wire, as
    /// `0b5714f`'s daemon did (`ONE_SOURCE_GOLDEN`). So does a daemon over
    /// a Retail-only install, which follows Forever's folder beside Retail's
    /// though that folder never holds a log: a source named by its folder
    /// only once a second one holds a log. It proves that scenario's frames
    /// and nothing wider. `WOWDPS_BLESS=1` rewrites the golden and then
    /// fails, so no run passes by blessing; a re-bless changes what every
    /// Retail client is sent, and its commit says why.
    #[test]
    fn one_source_frames_are_the_daemon_s_before_editions() {
        let one = one_source_scenario(Hub::with_sources(vec![SourceSetup {
            spec: Some(RETAIL_SPEC.to_string()),
            edition: Some(Edition::Retail),
        }]));
        let got: Vec<String> = one.iter().map(Frame::golden).collect();
        if std::env::var_os("WOWDPS_BLESS").is_some() {
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/golden/one_source_frames.txt"
            );
            let head: String = ONE_SOURCE_GOLDEN
                .lines()
                .take_while(|l| l.starts_with('#'))
                .map(|l| format!("{l}\n"))
                .collect();
            std::fs::write(path, head + &got.join("\n") + "\n").unwrap();
            panic!("WOWDPS_BLESS rewrote {path}: rerun without it, and say why in the commit");
        }
        let want: Vec<&str> = ONE_SOURCE_GOLDEN
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect();
        assert!(
            want.len() > 60,
            "the scenario says something: {}",
            want.len()
        );
        // One source, then a Retail-only install: Retail's folder and
        // Forever's, configured as production configures them, Forever's
        // never holding a log.
        for (what, frames) in [
            ("one source", one),
            ("an install", one_source_scenario(two_editions())),
        ] {
            let got: Vec<String> = frames.iter().map(Frame::golden).collect();
            for (i, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_eq!(g, w, "{what}: frame {i} moved: {}", frames[i].line());
            }
            assert_eq!(got.len(), want.len(), "{what}: the frame count moved");
        }
    }

    /// The scenario's Retail log, then Forever's first log landing in the
    /// second source while Retail's is read: from then the sources are
    /// named by their folders, and Retail's log — whose header names no
    /// edition — is Retail's by its folder. Its followers and its Retail
    /// pin are sent its list naming Retail at once, not on its next
    /// change.
    #[test]
    fn a_second_log_names_every_source_by_its_folder() {
        let mut hub = two_editions();
        let follower = hub.connect(1, ClientKind::Tui);
        hub.client(1, ClientMsg::watch(Cursor::List));
        let pinned = hub.connect(2, ClientKind::Tui);
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        // A Retail log whose header names no edition: before a second
        // source holds a log, it is named by nothing, as one source names
        // it.
        let header = "7/27/2026 20:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.5,PROJECT_ID,0";
        let pull = retail_pull(20);
        let mut text: Vec<String> = vec![header.to_string()];
        text.extend(pull);
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/wow/_retail_/Logs/WoWCombatLog-old.txt")),
        ));
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(text)));
        hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
        hub.tick();
        assert_eq!(hub.sources.engine(0).edition(), None, "named by nothing");
        assert_eq!(last_list(&drain(&follower)).map(|l| l.1), Some(None));
        drain(&pinned);

        // Forever's first log: both sources are named by their folders,
        // and Retail's list goes out naming Retail.
        deliver_log_to(&mut hub, 1, FOREVER, 0);
        assert_eq!(hub.sources.engine(0).edition(), Some(Edition::Retail));
        assert_eq!(hub.sources.engine(1).edition(), Some(Edition::Forever));
        let named = |rx: &Receiver<DaemonMsg>| {
            drain(rx).iter().any(|m| {
                matches!(
                    m,
                    DaemonMsg::SegmentList { entries, edition: Some(Edition::Retail), .. }
                        if !entries.is_empty()
                )
            })
        };
        assert!(named(&pinned), "the Retail pin is sent Retail's list");
        hub.handle(HubMsg::Tail(1, TailEvent::CaughtUp));
        hub.tick();
        assert_eq!(
            hub.sources.live(),
            1,
            "Forever's log, written last, is live: the follower moves"
        );

        // Named for good: Retail's next log is Retail's by its folder from
        // its switch, before its index lands.
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Switched(PathBuf::from("/wow/_retail_/Logs/WoWCombatLog-new.txt")),
        ));
        assert!(hub.sources.engine(0).awaiting_index());
        assert_eq!(hub.sources.engine(0).edition(), Some(Edition::Retail));
    }

    /// v49: an empty list is held only since the session's last Watch: one
    /// that leaves its pin (here for Retail's, on `Live`, which is sent no
    /// list) and comes back is sent its edition's empty list at the next
    /// move of the live edition, though it was sent the same one before.
    #[test]
    fn a_new_watch_forgets_the_empty_list_the_session_held() {
        let mut hub = two_editions();
        deliver_both(&mut hub);
        let rx = hub.connect(1, ClientKind::Tui);
        let watch = |hub: &mut Hub, cursor: Cursor, edition: Edition| {
            hub.client(
                1,
                ClientMsg::Watch {
                    cursor,
                    edition: EditionRef::Pin(edition),
                },
            );
        };
        watch(&mut hub, Cursor::List, Edition::Classic(2));
        assert!(matches!(
            drain(&rx).as_slice(),
            [DaemonMsg::SegmentList { entries, .. }] if entries.is_empty()
        ));
        watch(&mut hub, seg_cursor(SegmentRef::Live), Edition::Retail);
        watch(&mut hub, seg_cursor(SegmentRef::Live), Edition::Classic(2));
        drain(&rx);
        hub.handle(HubMsg::Game(Running::of(&[Edition::Retail])));
        assert_eq!(hub.sources.live(), 0, "the live edition moved");
        let got = drain(&rx);
        assert!(
            got.iter().any(|m| matches!(
                m,
                DaemonMsg::SegmentList { entries, edition: Some(Edition::Classic(2)), .. }
                    if entries.is_empty()
            )),
            "{got:?}"
        );
    }

    /// v49: a pin no source serves holds its edition's empty list from
    /// every source alike, so a move of the live edition sends it none.
    #[test]
    fn an_unserved_pin_is_not_resent_its_empty_list_on_a_move() {
        let mut hub = two_editions();
        let classic = hub.connect(1, ClientKind::Tui);
        hub.client(
            1,
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition: EditionRef::Pin(Edition::Classic(2)),
            },
        );
        deliver_both(&mut hub);
        drain(&classic);
        for running in [
            Running::of(&[Edition::Retail]),
            Running::of(&[Edition::Forever]),
            Running::of(&[Edition::Retail]),
        ] {
            let was = hub.sources.live();
            hub.handle(HubMsg::Game(running));
            assert_ne!(hub.sources.live(), was, "the live edition moved");
            let got = drain(&classic);
            assert!(got.is_empty(), "{got:?}");
        }
    }

    /// Each edition's log has its own engine: one's index, live meter and
    /// ids are untouched by the other's, ids never repeat across the two,
    /// and an id resolves in the source holding it whatever the session
    /// looks in. A rotation in one source retires its ids alone: Rotated
    /// for them, NotFound for an id never issued, and the other's resolve.
    #[test]
    fn each_edition_has_its_own_engine_and_one_id_counter() {
        let mut hub = two_editions();
        deliver_both(&mut hub);
        let retail = hub.sources.engine(0).list_ids();
        let forever = hub.sources.engine(1).list_ids();
        assert!(!retail.is_empty() && !forever.is_empty());
        assert!(
            retail.iter().all(|id| !forever.contains(id)),
            "{retail:?} {forever:?}"
        );
        assert_eq!(hub.sources.engine(0).edition(), Some(Edition::Retail));
        assert_eq!(hub.sources.engine(1).edition(), Some(Edition::Forever));
        assert_eq!(hub.sources.owner_of(retail[0]), Some(0));
        assert_eq!(hub.sources.owner_of(forever[0]), Some(1));

        // A Retail pin watching a Forever segment by id is answered from
        // Forever's source, the frame naming it.
        let rx = hub.connect(1, ClientKind::Tui);
        hub.client(
            1,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Id(forever[0])),
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        match drain(&rx).as_slice() {
            [
                DaemonMsg::Snapshot {
                    id: Some(id),
                    edition: Some(Edition::Forever),
                    source: Some(log),
                    ..
                },
            ] => {
                assert_eq!(*id, forever[0]);
                assert_eq!(log, "forever.txt");
            }
            other => panic!("{other:?}"),
        }

        // Forever's log rotates (the same fixture under another name).
        let dir = std::env::temp_dir().join(format!("wowdps-hub-rot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let next = dir.join("WoWCombatLog-110226_200000.txt");
        std::fs::copy(FOREVER, &next).unwrap();
        deliver_log_to(&mut hub, 1, next.to_str().unwrap(), 0);
        assert_eq!(
            hub.sources.engine(0).list_ids(),
            retail,
            "Retail's state kept"
        );
        let rotated = hub.sources.engine(1).list_ids();
        assert!(
            rotated
                .iter()
                .all(|id| !forever.contains(id) && !retail.contains(id))
        );
        assert_eq!(hub.sources.owner_of(forever[0]), None);
        assert_eq!(hub.sources.lost(forever[0]), LoadError::Rotated);
        assert_eq!(hub.sources.lost(SegmentId(1_000_000)), LoadError::NotFound);
        hub.tick();
        assert!(
            drain(&rx).iter().any(|m| matches!(
                m,
                DaemonMsg::LoadFailed { segment, error: LoadError::Rotated } if *segment == forever[0]
            )),
            "the rotated id fails as rotated"
        );
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Id(retail[0]))));
        assert!(matches!(
            drain(&rx).as_slice(),
            [DaemonMsg::Snapshot { id: Some(id), edition: Some(Edition::Retail), .. }] if *id == retail[0]
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Following is the live edition's source; a pin is its edition's, or
    /// the empty answer for an edition no source holds. With no game
    /// running the live edition is the one whose log was written last.
    /// `Status` lists both sources, the live one marked.
    #[test]
    fn a_session_follows_the_live_edition_or_its_pin() {
        let mut hub = two_editions();
        let sessions = [
            EditionRef::Follow,
            EditionRef::Pin(Edition::Retail),
            EditionRef::Pin(Edition::Forever),
            EditionRef::Pin(Edition::Classic(2)),
        ];
        let rxs: Vec<Receiver<DaemonMsg>> =
            (1..=4).map(|id| hub.connect(id, ClientKind::Tui)).collect();
        for (id, edition) in (1..).zip(sessions) {
            hub.client(
                id,
                ClientMsg::Watch {
                    cursor: Cursor::List,
                    edition,
                },
            );
        }
        deliver_both(&mut hub);
        assert_eq!(hub.sources.live(), 1, "Forever's log was written last");
        let lists: Vec<_> = rxs
            .iter()
            .map(|rx| last_list(&drain(rx)).unwrap())
            .collect();
        let every = |rows: &[Edition], e: Edition| !rows.is_empty() && rows.iter().all(|r| *r == e);
        assert!(every(&lists[0].0, Edition::Forever), "{:?}", lists[0]);
        assert_eq!(lists[0].1, Some(Edition::Forever));
        assert!(every(&lists[1].0, Edition::Retail), "{:?}", lists[1]);
        assert_eq!(lists[1].1, Some(Edition::Retail));
        assert_eq!(lists[1].2.as_deref(), Some("sample.txt"));
        assert!(every(&lists[2].0, Edition::Forever));
        assert_eq!(lists[3], (Vec::new(), Some(Edition::Classic(2)), None));

        hub.client(1, ClientMsg::GetStatus { req_id: 3 });
        match drain(&rxs[0]).as_slice() {
            [
                DaemonMsg::Status {
                    source, editions, ..
                },
            ] => {
                assert_eq!(source.as_deref(), Some("logs:/wow/_retail_/Logs"));
                let got: Vec<_> = editions
                    .iter()
                    .map(|e| (e.edition, e.log.clone(), e.live, e.running))
                    .collect();
                assert_eq!(
                    got,
                    [
                        (
                            Some(Edition::Retail),
                            Some("sample.txt".to_string()),
                            false,
                            false
                        ),
                        (
                            Some(Edition::Forever),
                            Some("forever.txt".to_string()),
                            true,
                            false
                        ),
                    ]
                );
            }
            other => panic!("{other:?}"),
        }
    }

    /// The live edition's transitions: the newest line with no game; the
    /// running game's when one runs (a following session moved, its frames
    /// naming the new edition); with both running, held while it has a pull
    /// open or for `HANDOFF_QUIET` after a move, else the newest line — and a
    /// pull opening in the edition not live draws no follower. With none
    /// running again, the newest line.
    #[test]
    fn the_live_edition_follows_the_running_game() {
        let mut hub = two_editions();
        let follower = hub.connect(1, ClientKind::Window);
        let retail = hub.connect(2, ClientKind::Tui);
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: seg_cursor(SegmentRef::Live),
                edition: EditionRef::Pin(Edition::Retail),
            },
        );
        deliver_both(&mut hub);
        hub.tick();
        assert_eq!(hub.sources.live(), 1);
        let got = drain(&follower);
        assert_eq!(last_list(&got).map(|l| l.1), Some(Some(Edition::Forever)));
        drain(&retail);

        // Retail's game starts: Retail is live, and the follower is sent
        // Retail's list — the frame that moves it.
        hub.handle(HubMsg::Game(Running::of(&[Edition::Retail])));
        assert_eq!(hub.sources.live(), 0);
        let got = drain(&follower);
        assert_eq!(
            last_list(&got).map(|l| (l.1, l.2)),
            Some((Some(Edition::Retail), Some("sample.txt".to_string())))
        );
        assert!(drain(&retail).is_empty(), "the pin did not move");

        // Both run: the watcher's move holds it.
        hub.handle(HubMsg::Game(Running::of(&[
            Edition::Retail,
            Edition::Forever,
        ])));
        assert_eq!(hub.sources.live(), 0, "both running: the move holds it");
        assert!(drain(&follower).is_empty());
        // Retail's log shows a line on the clock both share, newer than
        // Forever's: the hold over, nothing moves without Forever's pull.
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(300))));
        hub.sources.age_for_test(QUIET);
        hub.tick();
        assert_eq!(hub.sources.live(), 0, "Retail's line is the newest");
        drain(&follower);
        drain(&retail);
        // The hold over, a Forever pull opening, its lines the newest, moves
        // the live edition there on the tick: the follower is sent
        // Forever's list, which moves it, and then Forever's live fight; the
        // Retail pin hears nothing.
        hub.sources.age_for_test(QUIET);
        hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_pull(10))));
        assert_eq!(hub.sources.live(), 0, "the tick decides");
        hub.tick();
        assert_eq!(hub.sources.live(), 1);
        let got = drain(&follower);
        assert_eq!(last_list(&got).map(|l| l.1), Some(Some(Edition::Forever)));
        assert!(matches!(
            got.last(),
            Some(DaemonMsg::Snapshot { edition: Some(Edition::Forever), info, .. }) if info.live
        ));
        assert!(opened(&drain(&retail)).is_empty());

        // A Retail pull opening while Forever's is open moves nothing,
        // though its lines are newer: the Retail pin hears of it, the
        // follower not.
        hub.sources.age_for_test(QUIET);
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_pull_at(30))));
        hub.tick();
        assert_eq!(hub.sources.live(), 1, "Forever's pull is open");
        assert!(opened(&drain(&follower)).is_empty());
        assert_eq!(opened(&drain(&retail)).len(), 1);

        // Forever's pull ends: Retail's log shows the newest line, and so
        // Retail is live.
        hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_end(10))));
        hub.tick();
        assert_eq!(hub.sources.live(), 0);
        assert_eq!(
            last_list(&drain(&follower)).map(|l| l.1),
            Some(Some(Edition::Retail))
        );
        // Neither has a pull open: it stays while Retail's lines are the
        // newest, until the other's are.
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_end_at(30))));
        hub.tick();
        assert_eq!(hub.sources.live(), 0, "neither open: kept");
        hub.sources.age_for_test(QUIET);
        hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_pull(40))));
        hub.tick();
        assert_eq!(hub.sources.live(), 1);
        hub.sources.age_for_test(QUIET);
        hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_end(40))));
        hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_pull_at(50))));
        hub.tick();
        assert_eq!(hub.sources.live(), 0);
        assert_eq!(
            last_list(&drain(&follower)).map(|l| l.1),
            Some(Some(Edition::Retail))
        );

        // Forever quits, then Retail: Retail stays — its log shows the
        // newest line.
        hub.handle(HubMsg::Game(Running::of(&[Edition::Retail])));
        hub.handle(HubMsg::Game(Running::default()));
        assert_eq!(hub.sources.live(), 0);
        // Only Forever runs: Forever's.
        hub.handle(HubMsg::Game(Running::of(&[Edition::Forever])));
        assert_eq!(hub.sources.live(), 1);
        assert_eq!(
            last_list(&drain(&follower)).map(|l| l.1),
            Some(Some(Edition::Forever))
        );
    }

    /// v49: a session pinned to an edition no source serves holds an empty
    /// list, and its trash can tombstones nothing — not the live log's
    /// trash. A following session's does, as it always did.
    #[test]
    fn a_pin_no_source_serves_discards_no_trash() {
        let mut hub = Hub::new();
        let follower = hub.connect(1, ClientKind::Tui);
        let forever = hub.connect(2, ClientKind::Tui);
        hub.client(
            2,
            ClientMsg::Watch {
                cursor: Cursor::List,
                edition: EditionRef::Pin(Edition::Forever),
            },
        );
        hub.handle(HubMsg::Tail(
            0,
            TailEvent::Lines(vec![hit(0, 0), hit(10, 0)]),
        ));
        assert_eq!(hub.engine().list_rows().len(), 2, "closed trash + live");
        drain(&follower);
        drain(&forever);
        hub.client(2, ClientMsg::DiscardTrash);
        assert_eq!(
            hub.engine().list_rows().len(),
            2,
            "the Retail log's trash kept"
        );
        assert!(drain(&follower).is_empty() && drain(&forever).is_empty());
        hub.client(1, ClientMsg::DiscardTrash);
        assert_eq!(hub.engine().list_rows().len(), 1);
        drain(&forever);
        // A one-shot's `Live` in that edition is nobody's.
        hub.client(
            2,
            ClientMsg::GetLoadout {
                req_id: 3,
                segment: SegmentRef::Live,
                guid: "Player-1-A".to_string(),
            },
        );
        assert!(matches!(
            drain(&forever).as_slice(),
            [DaemonMsg::Loadout {
                req_id: 3,
                loadout: None,
                ..
            }]
        ));
    }

    /// v49: a slice whose log rotated away while it was parsed is no
    /// source's: dropped, never installed where nothing resolves it (one
    /// source installed it all the same, spending an LRU slot); the waiter
    /// is told its segment rotated away, as before.
    #[test]
    fn a_slice_of_a_rotated_log_is_dropped() {
        let mut hub = Hub::new();
        let rx = hub.connect(1, ClientKind::Window);
        let ids = hub.index_fixture();
        let boss = ids
            .iter()
            .copied()
            .zip(hub.engine().list_rows())
            .find(|(_, r)| r.kind == wowdps_model::SegmentKind::Encounter)
            .map(|(id, _)| id)
            .unwrap();
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Id(boss))));
        let req = hub.loader_rx.try_recv().expect("a load");
        deliver_log(&mut hub, FOREVER);
        drain(&rx);
        let text = wowdps_core::index::load_segment_text(&req.path, &req.meta).unwrap();
        hub.handle(HubMsg::Loaded {
            id: req.id,
            result: Ok(Box::new(text.meter())),
        });
        assert_eq!(hub.engine().resident(), 0, "no meter nothing resolves");
        assert!(
            drain(&rx).iter().any(|m| matches!(
                m,
                DaemonMsg::LoadFailed { segment, error: LoadError::Rotated } if *segment == boss
            )),
            "the waiter hears its segment rotated away"
        );
    }

    /// v49: with no game named, a move holds the live edition for
    /// `HANDOFF_QUIET` — the start-up pick of Forever's log, newest by its
    /// lines, is one — though the other log shows newer lines within it:
    /// two clients the watcher cannot name, writing in turn, move no
    /// follower on every line. Then the newest line wins, on the next tick,
    /// no event needed.
    #[test]
    fn with_no_game_a_move_holds_then_the_newest_line_wins() {
        let mut hub = two_editions();
        let follower = hub.connect(1, ClientKind::Tui);
        hub.client(1, ClientMsg::watch(Cursor::List));
        deliver_both(&mut hub);
        assert_eq!(hub.sources.live(), 1, "Forever's log shows the newest line");
        assert!(hub.sources.engine(1).line_ms() > hub.sources.engine(0).line_ms());
        drain(&follower);
        for t in 0..3 {
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(t))));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "the start-up move holds it");
        }
        let got = drain(&follower);
        assert!(
            got.iter().all(|m| !matches!(
                m,
                DaemonMsg::SegmentList { edition, .. } if *edition != Some(Edition::Forever)
            )),
            "the follower was not moved: {got:?}"
        );
        // The hold lapses: Retail's log shows the newest line, and the next
        // tick moves it there.
        hub.sources.age_for_test(Duration::from_secs(120));
        hub.tick();
        assert_eq!(hub.sources.live(), 0);
        assert_eq!(
            last_list(&drain(&follower)).map(|l| l.1),
            Some(Some(Edition::Retail))
        );
    }

    /// A line time `t` seconds past 10:00 on 11/2/2026, UTC−8: later than
    /// either fixture's last line, on one clock for both logs.
    fn stamp(t: u32) -> String {
        format!(
            "11/2/2026 {:02}:{:02}:{:02}.000-8",
            10 + t / 3600,
            t / 60 % 60,
            t % 60
        )
    }

    /// A buff landing out of combat in Retail's log at `t` ([`stamp`]):
    /// written, and no segment opened or extended.
    fn retail_buff(t: u32) -> Vec<String> {
        vec![format!(
            "{}  SPELL_AURA_APPLIED,Player-1168-0A1B2C02,\"Mirelle-Realm\",0x514,0x80000000,Player-1168-0A1B2C01,\"Ana-Realm\",0x511,0x80000000,17,\"Power Word: Shield\",0x2,BUFF,15600",
            stamp(t)
        )]
    }

    /// The same in Forever's log.
    fn forever_buff(t: u32) -> Vec<String> {
        vec![format!(
            "{}  SPELL_AURA_APPLIED,Player-6101-0A2B3C03,\"Tharokk-Emberfell-Nebula-US\",0x512,0x80000000,Player-6101-0A2B3C03,\"Tharokk-Emberfell-Nebula-US\",0x512,0x80000000,71,\"Defensive Stance\",0x1,BUFF",
            stamp(t)
        )]
    }

    /// A hit on a creature in Forever's log at `t` ([`stamp`]): out of an
    /// encounter, a trash pull opened or kept open.
    fn forever_hit(t: u32) -> Vec<String> {
        vec![format!(
            "{}  SPELL_DAMAGE,Player-6101-0A2B3C01,\"Velkris-Duskmantle-Nebula-US\",0x511,0x80000000,Creature-0-6101-48-2001-990002-0000A0B002,\"Murloc\",0xa48,0x80000000,1752,\"Sinister Strike\",0x1,Creature-0-6101-48-2001-990002-0000A0B002,0000000000000000,1170,1290,93,0,932,0,0,0,1,0,0,0,-507.79,-11.67,0,0.3814,22,120,125,-1,1,0,0,0,nil,nil,nil,ST",
            stamp(t)
        )]
    }

    /// `ms` (UTC, since the epoch) as a line's stamp written at UTC+0.
    fn utc_stamp(ms: i64) -> String {
        let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
        // Days since 1970-01-01 to the civil date.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!(
            "{m}/{d}/{y} {:02}:{:02}:{:02}.{:03}+0",
            rem / 3_600_000,
            rem / 60_000 % 60,
            rem / 1000 % 60,
            rem % 1000
        )
    }

    /// The daemon's clock now, in UTC milliseconds since the epoch.
    fn now_ms() -> i64 {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        i64::try_from(d.as_millis()).unwrap()
    }

    /// A second past `HANDOFF_QUIET`.
    const QUIET: Duration = Duration::from_secs(HANDOFF_QUIET.as_secs() + 1);

    /// Retail's log and Forever's caught up, Forever's live (written last),
    /// a following window on `Live`; both games running (`both`) or none
    /// seen; then `HANDOFF_QUIET` passed with neither log written. Every
    /// test of the live edition moving on log activity runs over both.
    fn playing_forever(both: bool) -> (Hub, Receiver<DaemonMsg>) {
        let mut hub = two_editions();
        let follower = hub.connect(1, ClientKind::Window);
        hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
        deliver_both(&mut hub);
        if both {
            hub.handle(HubMsg::Game(Running::of(&[
                Edition::Retail,
                Edition::Forever,
            ])));
        }
        hub.sources.age_for_test(QUIET);
        hub.tick();
        assert_eq!(hub.sources.live(), 1, "Forever's log was written last");
        drain(&follower);
        (hub, follower)
    }

    /// Whether `msgs` moved a follower to `edition`: a list naming it.
    fn moved_to(msgs: &[DaemonMsg], edition: Edition) -> bool {
        msgs.iter()
            .any(|m| matches!(m, DaemonMsg::SegmentList { edition: e, .. } if *e == Some(edition)))
    }

    /// `text` under `name` in a scratch folder of its own for this test.
    fn scratch_log(tag: &str, name: &str, text: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("wowdps-hub-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        let s = path.to_str().unwrap().to_string();
        (dir, s)
    }

    /// The player tabs from Forever to Retail with no pull open: Retail's
    /// log showing a line newer than Forever's makes Retail live on the next
    /// tick — the follower is moved, its frames naming Retail — however
    /// recently Forever's last line landed.
    #[test]
    fn tabbing_to_the_other_game_moves_the_live_edition_when_its_log_grows() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(0))));
            hub.sources.age_for_test(Duration::from_secs(5));
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(5))));
            assert_eq!(hub.sources.live(), 1, "both={both}: the tick decides");
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: Retail's line is newer");
            let got = drain(&follower);
            assert!(moved_to(&got, Edition::Retail), "both={both}: {got:?}");
            assert!(matches!(
                got.last(),
                Some(DaemonMsg::Snapshot {
                    edition: Some(Edition::Retail),
                    ..
                })
            ));
            hub.sources.age_for_test(QUIET);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(16))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: Retail is played");
        }
    }

    /// An open pull in the live edition holds it: the other's log showing
    /// newer lines never takes it, through minutes of the pull. Once the
    /// pull has ended, the other's newer lines do.
    #[test]
    fn the_other_log_growing_never_takes_live_from_a_pull() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_pull(10))));
            assert!(hub.sources.engine(1).pull_open());
            for k in 0..6 {
                hub.sources.age_for_test(Duration::from_secs(30));
                hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(602 + 30 * k))));
                hub.tick();
                assert_eq!(hub.sources.live(), 1, "both={both}: Forever's pull is open");
            }
            assert!(!moved_to(&drain(&follower), Edition::Retail));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_end(10))));
            hub.sources.age_for_test(QUIET);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(900))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: the pull is over");
        }
    }

    /// With both logs quiet nothing moves, tick after tick.
    #[test]
    fn with_both_logs_quiet_the_live_edition_stays() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            for _ in 0..10 {
                hub.sources.age_for_test(Duration::from_secs(30));
                hub.tick();
                assert_eq!(hub.sources.live(), 1, "both={both}");
            }
            assert!(!moved_to(&drain(&follower), Edition::Retail));
        }
    }

    /// Both logs growing within one tick, no pull open in either: the tick
    /// picks the log whose line is newer, whichever's landed first; an equal
    /// time keeps the live edition.
    #[test]
    fn two_logs_growing_in_one_tick_pick_the_newer_line() {
        for both in [true, false] {
            for (retail, forever, retail_first) in [
                (101, 100, true),
                (101, 100, false),
                (100, 101, true),
                (100, 101, false),
                (100, 100, true),
                (100, 100, false),
            ] {
                let (mut hub, _follower) = playing_forever(both);
                let lines = [
                    HubMsg::Tail(0, TailEvent::Lines(retail_buff(retail))),
                    HubMsg::Tail(1, TailEvent::Lines(forever_buff(forever))),
                ];
                let [r, f] = lines;
                let order = if retail_first { [r, f] } else { [f, r] };
                for msg in order {
                    hub.handle(msg);
                }
                hub.tick();
                let newer = if retail > forever { 0 } else { 1 };
                assert_eq!(
                    hub.sources.live(),
                    newer,
                    "both={both} retail={retail} forever={forever} retail_first={retail_first}"
                );
            }
        }
    }

    /// Tabbed to Retail with no pull: Retail is live. A pull opening in
    /// Forever, its lines newer, takes it back on the tick once the move to
    /// Retail is `HANDOFF_QUIET` old — the follower is moved by Forever's
    /// list — and Retail's log growing during that pull moves nothing.
    #[test]
    fn a_pull_opening_wins_the_live_edition_back() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(0))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}");
            drain(&follower);
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_pull(10))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: the move holds it");
            hub.sources.age_for_test(QUIET);
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: Forever's pull");
            let got = drain(&follower);
            assert!(moved_to(&got, Edition::Forever), "both={both}: {got:?}");
            hub.sources.age_for_test(QUIET);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(700))));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: its pull holds it");
        }
    }

    /// A log the game creates in the other edition's folder shows its
    /// newer lines: the live edition moves to it on the next tick. The same
    /// log re-read (a truncation), a switch to an older log, and a log
    /// copied in (new by its modified time, old by its lines) show none.
    #[test]
    fn a_new_log_in_the_other_folder_moves_the_live_edition() {
        let sample = std::fs::read_to_string(FIXTURE).unwrap();
        let (dir, older) = scratch_log("newlog", "WoWCombatLog-072626_200000.txt", &sample);
        let copied = dir.join("WoWCombatLog-072726_210000.txt");
        std::fs::write(&copied, &sample).unwrap();
        let copied = copied.to_str().unwrap();
        let new = dir.join("WoWCombatLog-110226_100000.txt");
        std::fs::write(&new, format!("{sample}{}\n", retail_buff(0)[0])).unwrap();
        let new = new.to_str().unwrap();
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            deliver_log_to(&mut hub, 0, FIXTURE, 0);
            hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: the same log re-read");
            deliver_log_to(&mut hub, 0, &older, 3_600_000);
            hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: an older log");
            deliver_log_to(&mut hub, 0, copied, 0);
            hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: a log copied in");
            assert!(!moved_to(&drain(&follower), Edition::Retail));
            deliver_log_to(&mut hub, 0, new, 0);
            assert_eq!(hub.sources.live(), 1, "both={both}: the tick decides");
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: a new log");
            assert!(moved_to(&drain(&follower), Edition::Retail));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder's first log, whether the folder held none before (the
    /// game's new log) or it is there at start: the newest line wins, so
    /// with both games running the live edition moves to Forever's, its
    /// lines newer than Retail's, on the next tick.
    #[test]
    fn a_first_log_in_an_empty_folder_moves_the_live_edition() {
        for waited in [true, false] {
            let mut hub = two_editions();
            let follower = hub.connect(1, ClientKind::Window);
            hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
            hub.handle(HubMsg::Game(Running::of(&[
                Edition::Retail,
                Edition::Forever,
            ])));
            deliver_log_to(&mut hub, 0, FIXTURE, 0);
            hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            if waited {
                hub.handle(HubMsg::Tail(1, TailEvent::Waiting));
            }
            hub.sources.age_for_test(QUIET);
            hub.tick();
            assert_eq!(hub.sources.live(), 0);
            drain(&follower);
            deliver_log_to(&mut hub, 1, FOREVER, 0);
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "waited={waited}");
            assert!(moved_to(&drain(&follower), Edition::Forever));
        }
    }

    /// The game left behind flushes late: after the player tabbed from
    /// Forever to Retail, Forever's log grows with lines written before he
    /// left — older than Retail's newest. They move nothing, on the tick or
    /// later with no game seen, however long Retail's log is quiet. The
    /// player going back to Forever does.
    #[test]
    fn a_late_flush_of_the_game_left_moves_nothing() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(20))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: tabbed to Retail");
            drain(&follower);
            hub.sources.age_for_test(Duration::from_secs(20));
            for t in 0..5 {
                hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(t))));
            }
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: lines from before");
            for _ in 0..4 {
                hub.sources.age_for_test(Duration::from_secs(60));
                hub.tick();
                assert_eq!(
                    hub.sources.live(),
                    0,
                    "both={both}: written last by a flush"
                );
            }
            assert!(!moved_to(&drain(&follower), Edition::Forever));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(300))));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: back in Forever");
        }
    }

    /// The game left behind exits, flushing what it held: lines older than
    /// the played game's newest. With both games running, nothing moves
    /// before the watcher sees it gone, nor after; with none seen, nothing
    /// moves however long after.
    #[test]
    fn an_exit_flush_of_the_game_left_moves_nothing() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(600))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: tabbed to Retail");
            drain(&follower);
            hub.sources.age_for_test(Duration::from_secs(240));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(590))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: the exit flush");
            if both {
                hub.handle(HubMsg::Game(Running::of(&[Edition::Retail])));
                assert_eq!(hub.sources.live(), 0);
            }
            for _ in 0..4 {
                hub.sources.age_for_test(Duration::from_secs(60));
                hub.tick();
                assert_eq!(hub.sources.live(), 0, "both={both}");
            }
            assert!(!moved_to(&drain(&follower), Edition::Forever));
        }
    }

    /// The other log's growth must be later, by its newest line's own time,
    /// than the live log's newest: an equal time moves nothing, a later one
    /// does.
    #[test]
    fn a_growth_at_the_live_log_s_own_time_moves_nothing() {
        for both in [true, false] {
            let (mut hub, _follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(0))));
            hub.sources.age_for_test(QUIET);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(0))));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: the same second");
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(1))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: a second later");
        }
    }

    /// Lines that carry no comparable time — none at all, or the legacy
    /// format with no year and no offset — grow a log but never move the
    /// live edition, on the tick or later with no game seen. A line with one
    /// does.
    #[test]
    fn a_growth_with_no_line_time_moves_nothing() {
        let legacy = "11/2 10:00:05.000  SPELL_AURA_APPLIED,Player-1168-0A1B2C02,\"Mirelle-Realm\",0x514,0x80000000,Player-1168-0A1B2C01,\"Ana-Realm\",0x511,0x80000000,17,\"Power Word: Shield\",0x2,BUFF,15600";
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            let before = hub.sources.engine(0).line_ms();
            for batch in [vec!["no time here".to_string()], vec![legacy.to_string()]] {
                hub.handle(HubMsg::Tail(0, TailEvent::Lines(batch)));
                assert_eq!(hub.sources.engine(0).line_ms(), before, "no time shown");
                hub.tick();
                assert_eq!(hub.sources.live(), 1, "both={both}");
            }
            for _ in 0..3 {
                hub.sources.age_for_test(Duration::from_secs(60));
                hub.tick();
                assert_eq!(hub.sources.live(), 1, "both={both}");
            }
            assert!(!moved_to(&drain(&follower), Edition::Retail));
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(0))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: a line with a time");
        }
    }

    /// The live log rotates — its game starts a new file — or is truncated
    /// and re-read: the live edition stays with it, whether a game is seen
    /// or not, though its log was quiet past the trash gap and the other's
    /// was written an hour before. No frame moves the follower to the other
    /// edition, not even between the switch and the new log's index.
    #[test]
    fn a_rotation_or_truncation_of_the_live_log_keeps_it_live() {
        let forever = std::fs::read_to_string(FOREVER).unwrap();
        let text = format!("{forever}{}\n", forever_buff(0)[0]);
        let (dir, next) = scratch_log("rotate", "WoWCombatLog-110226_100000.txt", &text);
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.sources.age_for_test(Duration::from_secs(70));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}");
            for (what, path) in [
                ("a new log", next.as_str()),
                ("a truncation", next.as_str()),
            ] {
                let (switched, index, seeds) = log_parts(path);
                for msg in [switched, index, seeds] {
                    let HubMsg::Tail(_, ev) = msg else {
                        panic!("log_parts gives tail events")
                    };
                    hub.handle(HubMsg::Tail(1, ev));
                    assert_eq!(hub.sources.live(), 1, "both={both}: {what}");
                }
                hub.handle(HubMsg::Tail(1, TailEvent::CaughtUp));
                hub.tick();
                hub.sources.age_for_test(Duration::from_secs(9));
                hub.tick();
                assert_eq!(hub.sources.live(), 1, "both={both}: {what}");
            }
            assert!(!moved_to(&drain(&follower), Edition::Retail), "both={both}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RV-A: Forever fought trash and the player tabbed to Retail, which is
    /// live. Forever's late flush lands in its log's open trailing trash
    /// pull, holding it open by arrival — but its lines are older than
    /// Retail's newest: they take nothing, on the line, on the tick or later
    /// with no game seen. The player going back to Forever does.
    #[test]
    fn a_late_flush_into_an_open_trash_pull_takes_nothing() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_hit(0))));
            hub.sources.age_for_test(Duration::from_secs(70));
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(70))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: tabbed to Retail");
            drain(&follower);
            hub.sources.age_for_test(Duration::from_secs(20));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(30))));
            assert!(hub.sources.engine(1).pull_open(), "held open by arrival");
            assert_eq!(hub.sources.live(), 0, "both={both}: on the line");
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: on the tick");
            for _ in 0..3 {
                hub.sources.age_for_test(Duration::from_secs(60));
                hub.tick();
                assert_eq!(hub.sources.live(), 0, "both={both}: later");
            }
            assert!(!moved_to(&drain(&follower), Edition::Forever));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(400))));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: back in Forever");
        }
    }

    /// RV-B: Retail is live; Forever's late flush opens a trash pull of its
    /// own, with lines older than Retail's newest. The pull it opens takes
    /// nothing, then or while it is held open.
    #[test]
    fn a_late_flush_that_opens_a_pull_takes_nothing() {
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(200))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: tabbed to Retail");
            drain(&follower);
            let mut flush = forever_hit(100);
            flush.extend(forever_buff(150));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(flush)));
            assert!(hub.sources.engine(1).pull_open(), "a pull opened");
            assert_eq!(hub.sources.live(), 0, "both={both}: on the line");
            for wait in [30, 35] {
                hub.sources.age_for_test(Duration::from_secs(wait));
                hub.tick();
                assert_eq!(hub.sources.live(), 0, "both={both}");
            }
            assert!(!moved_to(&drain(&follower), Edition::Forever));
        }
    }

    /// RV-C: Retail is live, and its log rotates to a new file still empty.
    /// Retail's newest line time stays what its last log showed, so the
    /// exit flush of the game left, older than that, takes nothing.
    #[test]
    fn a_rotation_to_an_empty_log_keeps_the_newest_line_time() {
        let (dir, empty) = scratch_log("empty", "WoWCombatLog-110226_101000.txt", "");
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(600))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: tabbed to Retail");
            let newest = hub.sources.engine(0).line_ms();
            assert!(newest.is_some());
            drain(&follower);
            deliver_log_to(&mut hub, 0, &empty, 0);
            hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: after the rotation");
            assert_eq!(hub.sources.engine(0).line_ms(), newest, "kept");
            hub.sources.age_for_test(Duration::from_secs(20));
            hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(590))));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: the old game's flush");
            for _ in 0..3 {
                hub.sources.age_for_test(Duration::from_secs(60));
                hub.tick();
                assert_eq!(hub.sources.live(), 0, "both={both}: later");
            }
            assert!(!moved_to(&drain(&follower), Edition::Forever));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// RV-D: the live log's newest line is dated an hour past the daemon's
    /// clock (a clock run fast, a log from another host). It counts as a
    /// minute past the clock when it was shown: within that minute the other
    /// game's current lines do not pass it; once it has gone by they take
    /// the live edition, and do not wait the hour.
    #[test]
    fn a_line_dated_ahead_of_the_clock_holds_the_live_edition_a_minute_at_most() {
        let ahead = now_ms() + 3_600_000;
        assert_eq!(
            crate::engine::line_utc_ms(&format!("{}  ZONE_CHANGE,1,\"x\",0", utc_stamp(ahead))),
            Some(ahead)
        );
        let at = |line: Vec<String>, ms: i64| -> Vec<String> {
            line.iter()
                .map(|l| l.replacen(&stamp(0), &utc_stamp(ms), 1))
                .collect()
        };
        for both in [true, false] {
            let (mut hub, follower) = playing_forever(both);
            hub.handle(HubMsg::Tail(
                1,
                TailEvent::Lines(at(forever_buff(0), ahead)),
            ));
            let shown = hub.sources.engine(1).line_ms().unwrap();
            assert!(shown < ahead && shown <= now_ms() + 60_000, "{shown}");
            hub.sources.age_for_test(QUIET);
            hub.handle(HubMsg::Tail(
                0,
                TailEvent::Lines(at(retail_buff(0), now_ms())),
            ));
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: within the minute");
            // The minute goes by.
            hub.sources
                .engine_mut(1)
                .line_shown_for_test(Duration::from_secs(61));
            hub.handle(HubMsg::Tail(
                0,
                TailEvent::Lines(at(retail_buff(0), now_ms())),
            ));
            hub.tick();
            assert_eq!(hub.sources.live(), 0, "both={both}: not held the hour");
            assert!(moved_to(&drain(&follower), Edition::Retail));
        }
    }

    /// R4-1: at start-up a tick lands between Retail's `Switched` and its
    /// index, Retail started but with nothing to rank it by yet: the live
    /// edition goes to Forever, but that is no move, so it holds nothing —
    /// Retail's index lands, its line the newest, and the next tick has it.
    #[test]
    fn a_tick_before_a_source_s_first_index_holds_nothing() {
        let sample = std::fs::read_to_string(FIXTURE).unwrap();
        let (dir, newer) = scratch_log(
            "unindexed",
            "WoWCombatLog-110226_100000.txt",
            &format!("{sample}{}\n", retail_buff(0)[0]),
        );
        for both in [true, false] {
            let mut hub = two_editions();
            let follower = hub.connect(1, ClientKind::Window);
            hub.client(1, ClientMsg::watch(seg_cursor(SegmentRef::Live)));
            if both {
                hub.handle(HubMsg::Game(Running::of(&[
                    Edition::Retail,
                    Edition::Forever,
                ])));
            }
            deliver_log_to(&mut hub, 1, FOREVER, 0);
            hub.handle(HubMsg::Tail(1, TailEvent::CaughtUp));
            let (switched, index, seeds) = log_parts(&newer);
            hub.handle(switched);
            hub.tick();
            assert_eq!(hub.sources.live(), 1, "both={both}: Retail has no rank yet");
            hub.handle(index);
            hub.handle(seeds);
            hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
            hub.tick();
            assert_eq!(
                hub.sources.live(),
                0,
                "both={both}: no hold, the newest line"
            );
            assert!(moved_to(&drain(&follower), Edition::Retail));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// R4-4: the rule's fourth sentence. A source whose log has shown no
    /// timed line (an empty log) is ranked by its log's modified time
    /// against the other's newest line time: modified before Forever's last
    /// line, Forever is live; modified after it, Retail is.
    #[test]
    fn a_source_with_no_timed_line_is_ranked_by_its_modified_time() {
        let (dir, empty) = scratch_log("untimed", "WoWCombatLog-110226_100000.txt", "");
        let forever_ms = std::fs::read_to_string(FOREVER)
            .unwrap()
            .lines()
            .last()
            .and_then(crate::engine::line_utc_ms)
            .unwrap();
        // Ages that put Retail's modified time an hour before Forever's last
        // line, and now.
        let before = u64::try_from(now_ms() - forever_ms + 3_600_000).unwrap();
        for both in [true, false] {
            for (age, live) in [(before, 1), (0, 0)] {
                let mut hub = two_editions();
                if both {
                    hub.handle(HubMsg::Game(Running::of(&[
                        Edition::Retail,
                        Edition::Forever,
                    ])));
                }
                deliver_log_to(&mut hub, 1, FOREVER, 0);
                hub.handle(HubMsg::Tail(1, TailEvent::CaughtUp));
                deliver_log_to(&mut hub, 0, &empty, age);
                hub.handle(HubMsg::Tail(0, TailEvent::CaughtUp));
                hub.tick();
                assert_eq!(hub.sources.engine(0).line_ms(), None, "no timed line");
                assert_eq!(hub.sources.live(), live, "both={both} age={age}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bound: lines alone, no pull opening — two logs written in turn,
    /// however they alternate — never move the live edition twice within
    /// `HANDOFF_QUIET`, the move's hold. Strict turns 9 s apart move it every
    /// third turn, 27 s apart: the turn after a move is held, the next is
    /// the live log's own; 11 s apart, every turn to the other log does, 11 s
    /// apart. A pseudo-random walk of lines and gaps (fixed seeds), each
    /// line timed when it lands, holds the bound too. Every move is counted,
    /// on a line or on the tick. With both games running and with none seen.
    #[test]
    fn lines_alone_move_the_live_edition_at_most_once_per_handoff_quiet() {
        // Each step: `gap` passes, then lines land in Retail's log and in
        // Forever's as `lines` says, then a tick. The simulated time of
        // every move.
        fn walk(both: bool, steps: &[(Duration, [bool; 2])]) -> Vec<Duration> {
            let (mut hub, _follower) = playing_forever(both);
            let mut now = Duration::ZERO;
            let mut moves = Vec::new();
            let mut live = hub.sources.live();
            let mut saw = |hub: &Hub, now: Duration, moves: &mut Vec<Duration>| {
                if hub.sources.live() != live {
                    live = hub.sources.live();
                    moves.push(now);
                }
            };
            for (gap, lines) in steps {
                hub.sources.age_for_test(*gap);
                now += *gap;
                let t = now.as_secs() as u32;
                if lines[0] {
                    hub.handle(HubMsg::Tail(0, TailEvent::Lines(retail_buff(t))));
                    saw(&hub, now, &mut moves);
                }
                if lines[1] {
                    hub.handle(HubMsg::Tail(1, TailEvent::Lines(forever_buff(t))));
                    saw(&hub, now, &mut moves);
                }
                hub.tick();
                saw(&hub, now, &mut moves);
            }
            moves
        }
        // Strict turns, the live log's line first.
        let turns = |gap: u64| -> Vec<(Duration, [bool; 2])> {
            (0..40)
                .map(|k| (Duration::from_secs(gap), [k % 2 == 1, k % 2 == 0]))
                .collect()
        };
        for both in [true, false] {
            let moves = walk(both, &turns(9));
            assert_eq!(moves.len(), 13, "both={both}: every third turn");
            assert!(
                moves
                    .windows(2)
                    .all(|w| w[1] - w[0] == Duration::from_secs(27))
            );
            let moves = walk(both, &turns(11));
            assert_eq!(moves.len(), 39, "both={both}: every turn but the first");
            assert!(
                moves
                    .windows(2)
                    .all(|w| w[1] - w[0] == Duration::from_secs(11))
            );

            let mut total = 0;
            for seed in 1..=16u64 {
                let mut x = seed;
                let mut next = move || {
                    x = x
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    x >> 33
                };
                let gaps = [0, 1, 3, 9, 11, 20, 70];
                let steps: Vec<(Duration, [bool; 2])> = (0..200)
                    .map(|_| {
                        let gap = gaps[(next() % gaps.len() as u64) as usize];
                        let which = next() % 4;
                        (Duration::from_secs(gap), [which & 1 == 1, which & 2 == 2])
                    })
                    .collect();
                let moves = walk(both, &steps);
                assert!(
                    moves.windows(2).all(|w| w[1] - w[0] >= HANDOFF_QUIET),
                    "both={both} seed={seed}: {moves:?}"
                );
                total += moves.len();
            }
            assert!(total > 16, "both={both}: the walks moved it ({total})");
        }
    }

    /// A game the watcher cannot name, or of an edition no source follows,
    /// counts for every source, as any game did with one source: a daemon
    /// of one Retail source reads a Forever client as its game running, as
    /// it always has. With both followed, each source's game is its own.
    #[test]
    fn a_game_no_source_is_of_counts_for_every_source() {
        let mut hub = Hub::new();
        deliver_log(&mut hub, FIXTURE);
        hub.handle(HubMsg::Game(Running::of(&[Edition::Forever])));
        assert!(hub.engine().game_running);
        hub.handle(HubMsg::Game(Running::unnamed()));
        assert!(hub.engine().game_running);
        hub.handle(HubMsg::Game(Running::default()));
        assert!(!hub.engine().game_running);

        // An install whose Forever folder holds no log reads it the same
        // way: no source is Forever's yet.
        let mut hub = two_editions();
        deliver_log(&mut hub, FIXTURE);
        hub.handle(HubMsg::Game(Running::of(&[Edition::Forever])));
        assert!(hub.sources.engine(0).game_running);
        assert_eq!(hub.sources.live(), 0);

        let mut hub = two_editions();
        deliver_both(&mut hub);
        hub.handle(HubMsg::Game(Running::of(&[Edition::Forever])));
        assert!(!hub.sources.engine(0).game_running);
        assert!(hub.sources.engine(1).game_running);
        hub.handle(HubMsg::Game(Running::unnamed()));
        assert!(hub.sources.engine(0).game_running && hub.sources.engine(1).game_running);
        assert_eq!(hub.sources.live(), 1, "a game it cannot name moves nothing");
    }

    /// The header decides, not the folder: a Forever log found in Retail's
    /// folder makes that source Forever's while it follows that log — a
    /// Forever pin is answered from it, a Retail pin is sent Retail's empty
    /// list (no other source holds a Retail log) — and a source whose
    /// folder has never held a log is not listed in `Status`.
    #[test]
    fn a_log_is_filed_by_its_header_not_its_folder() {
        let mut hub = two_editions();
        let retail = hub.connect(1, ClientKind::Tui);
        let forever = hub.connect(2, ClientKind::Tui);
        for (id, e) in [(1, Edition::Retail), (2, Edition::Forever)] {
            hub.client(
                id,
                ClientMsg::Watch {
                    cursor: Cursor::List,
                    edition: EditionRef::Pin(e),
                },
            );
        }
        assert_eq!(hub.sources.engine(0).edition(), None, "no log yet");
        deliver_log_to(&mut hub, 0, FOREVER, 0);
        assert_eq!(hub.sources.engine(0).edition(), Some(Edition::Forever));
        assert_eq!(
            last_list(&drain(&retail)),
            Some((Vec::new(), Some(Edition::Retail), None))
        );
        let (rows, edition, log) = last_list(&drain(&forever)).unwrap();
        assert!(!rows.is_empty() && rows.iter().all(|r| *r == Edition::Forever));
        assert_eq!(
            (edition, log.as_deref()),
            (Some(Edition::Forever), Some("forever.txt"))
        );

        hub.client(1, ClientMsg::GetStatus { req_id: 1 });
        match drain(&retail).as_slice() {
            [DaemonMsg::Status { editions, .. }] => {
                assert_eq!(editions.len(), 1, "Forever's folder holds no log yet");
                assert_eq!(editions[0].edition, Some(Edition::Forever));
                assert!(editions[0].live);
            }
            other => panic!("{other:?}"),
        }
    }

    /// v46: an `ImportReplay` the history thread never takes is answered
    /// `Refused` here, with why — the store's error when it is off, "busy"
    /// when the queue is full — and a busy refusal ticks no `dropped`.
    #[test]
    fn a_refused_import_is_answered_with_why() {
        let import = || HistoryReq::ImportReplay {
            session: 1,
            req_id: 9,
            card: Box::default(),
            replay: Vec::new(),
        };
        let refusal = |rx: &Receiver<DaemonMsg>| match rx.try_recv() {
            Ok(DaemonMsg::History {
                req_id: 9,
                answer: HistoryAnswer::Refused { reason },
            }) => reason,
            other => panic!("{other:?}"),
        };

        let (mut s, rx) = session(1);
        forward_history(
            &HistoryLink::disabled("history off for the test"),
            &mut s,
            import(),
        );
        assert_eq!(refusal(&rx), "history off for the test");

        let (link, _queue) = HistoryLink::bounded(1);
        assert!(
            link.send(HistoryReq::Sweep(PathBuf::from("/nowhere")))
                .is_ok()
        );
        let (mut s, rx) = session(2);
        forward_history(&link, &mut s, import());
        assert!(refusal(&rx).contains("busy"));
        assert_eq!(link.status().dropped, 0);
    }
}
