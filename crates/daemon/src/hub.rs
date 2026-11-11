//! The hub thread: owns the engine and the session table, applies client
//! messages, and on each tick rebuilds snapshots for watched cursors only,
//! pushing the ones that changed. Snapshot rate is capped by the tick.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender};
use std::time::{Duration, Instant};

use wowdps_core::index::SegmentMeta;
use wowdps_core::model::{Meter, SegmentId};
use wowdps_core::tail::TailEvent;
use wowdps_proto::{
    ClientKind, ClientMsg, Cursor, DaemonMsg, HistoryAnswer, HistoryQuery, LoadError, Night,
    PROTO_VERSION, SegmentRef,
};

use crate::engine::{Built, Engine, EngineEvent, ListKey, LoadoutBuilt};
use crate::history::{Ask, HistoryLink, HistoryReq, LogRef};
use crate::loader::{LoadReply, LoadReq};
use crate::overlay::{Cmd, Supervisor};
use crate::session::Session;

pub enum HubMsg {
    Tail(TailEvent),
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
    Game(bool),
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
    /// Canonical display of what this daemon tails (`spec_display`), reported
    /// in `Status` so a client with `--file`/`--logs` can detect a conflict.
    pub source_spec: Option<String>,
}

/// Runs until `Shutdown`, idle-exit, or every sender hanging up.
pub fn run(
    rx: Receiver<HubMsg>,
    loader: Sender<LoadReq>,
    mut supervisor: Supervisor,
    opts: HubOptions,
    history: HistoryLink,
) {
    let mut engine = Engine::new();
    let mut sessions: Vec<Session> = Vec::new();
    let mut last_list = ListKey::default();
    let mut game_running = false;
    let mut shutdown = false;
    let mut idle_since: Option<Instant> = Some(Instant::now());
    let mut last_tick = Instant::now();

    while !shutdown {
        let timeout = opts.tick.saturating_sub(last_tick.elapsed());
        // v35: whose rows are the reader's, as the history thread last
        // resolved it — a refcount, read before anything is built.
        engine.mine = history.mine();
        match rx.recv_timeout(timeout) {
            Ok(msg) => handle(
                msg,
                &mut engine,
                &mut sessions,
                &loader,
                &mut supervisor,
                &opts,
                &mut last_list,
                &mut game_running,
                &mut shutdown,
                &history,
            ),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if last_tick.elapsed() >= opts.tick {
            last_tick = Instant::now();
            for s in sessions.iter_mut() {
                push_cursor(s, &mut engine, &loader, game_running);
            }
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
    engine: &mut Engine,
    sessions: &mut Vec<Session>,
    loader: &Sender<LoadReq>,
    supervisor: &mut Supervisor,
    opts: &HubOptions,
    last_list: &mut ListKey,
    game_running: &mut bool,
    shutdown: &mut bool,
    history: &HistoryLink,
) {
    match msg {
        HubMsg::Tail(ev) => {
            // A switch to a newer log retires the one we leave: its open
            // tail (an abandoned pull, the raid night's Σ) is final now and
            // imports as it would on a restart, so a lingering daemon never
            // waits for one. The first `Switched` has nothing to retire.
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
            let mut events = Vec::new();
            engine.on_tail(ev, &mut events);
            for ev in events {
                match ev {
                    // v49: to the sessions this source answers — a pin to
                    // another edition has no fight starting here.
                    EngineEvent::Opened(id) => {
                        for s in sessions.iter_mut().filter(|s| engine.serves(s.edition)) {
                            s.push_control(DaemonMsg::SegmentOpened { id });
                        }
                    }
                    // One clone and a try_send: every other byte of history
                    // work happens on its own thread.
                    EngineEvent::Closed(id) => {
                        if !history.enabled() {
                            continue;
                        }
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
            // session, not just List watchers. Off-list navigation resolves
            // neighbors by id, and a segment that opened *and closed* inside
            // one flush burst never gets a `SegmentOpened` of its own
            // (`Opened` only covers a batch's still-open tail). v49: so did
            // the source's edition — a new log's seed lines name it after
            // its index has landed — and the list names it, its rows wear
            // it, and a session pinned to it is sent it only now.
            let key = engine.list_key();
            if key != *last_list {
                *last_list = key;
                broadcast_list(engine, sessions, *game_running);
            }
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
                    push_cursor(s, engine, loader, *game_running);
                }
                ClientMsg::GetStatus { req_id } => {
                    let clients = sessions.len() as u32;
                    let source = opts.source_spec.clone();
                    let overlay = supervisor.state();
                    let history = history.status();
                    // v49: every edition's source — today the one.
                    let editions = vec![engine.edition_source(source.clone(), *game_running)];
                    let Some(s) = sessions.iter_mut().find(|s| s.id == id) else {
                        return;
                    };
                    s.push_control(DaemonMsg::Status {
                        req_id,
                        game_running: *game_running,
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
                // R11: the trash can. The id table shrinks: broadcast the
                // new list to every session, exactly like a tail change.
                ClientMsg::DiscardTrash => {
                    engine.discard_trash();
                    let key = engine.list_key();
                    if key != *last_list {
                        *last_list = key;
                        broadcast_list(engine, sessions, *game_running);
                    }
                }
                // v19: a one-shot, answered like GetStatus when the segment
                // is warm; a cold segment parks the request behind the
                // loader and `HubMsg::Loaded` answers it.
                ClientMsg::GetLoadout {
                    req_id,
                    segment,
                    guid,
                } => match engine.loadout_in(s.edition, segment, &guid) {
                    LoadoutBuilt::Ready(loadout) => {
                        s.push_control(DaemonMsg::Loadout {
                            req_id,
                            guid,
                            loadout,
                        });
                    }
                    LoadoutBuilt::Loading(seg_id, meta) => {
                        s.pending_loadouts.push((req_id, seg_id, guid));
                        request_load(engine, loader, seg_id, meta);
                    }
                },
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
            engine.loading.remove(&id);
            let loaded_ok = result.is_ok();
            match result {
                Ok(meter) => {
                    engine.install_loaded(id, *meter);
                    if engine.history_pending.remove(&id)
                        && history.enabled()
                        && let Some(fight) = engine.take_closed(id)
                    {
                        let _ = history.send(HistoryReq::Store(Box::new(fight)));
                    }
                    // Whoever is waiting on this segment gets it now, not at
                    // the next tick.
                    for s in sessions.iter_mut() {
                        if cursor_wants(s.cursor.as_ref(), id) {
                            push_cursor(s, engine, loader, *game_running);
                        }
                    }
                }
                Err(e) => {
                    // The sweep after the next start stores it from the file.
                    engine.history_pending.remove(&id);
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
                    let loadout = match (loaded_ok, engine.loadout(SegmentRef::Id(sid), &guid)) {
                        (true, LoadoutBuilt::Ready(l)) => l,
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
        HubMsg::Game(g) => {
            *game_running = g;
            engine.game_running = g;
            let cmds = supervisor.on_game(g);
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

/// The id table changed shape: the list to every session this source
/// answers, not just List watchers, each stamped with the edition it is
/// answered in (v49). A session pinned to another edition keeps the list
/// it was answered — its edition's, which did not change.
fn broadcast_list(engine: &Engine, sessions: &mut [Session], game_running: bool) {
    let list = engine.build_list(game_running);
    for s in sessions.iter_mut() {
        if let Some(list) = engine.scoped(s.edition, list.clone()) {
            s.push_list(list);
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

/// Build and (dedup-)push whatever `s` is watching.
fn push_cursor(s: &mut Session, engine: &mut Engine, loader: &Sender<LoadReq>, game: bool) {
    let Some(cursor) = s.cursor.as_ref() else {
        return;
    };
    // v49: in the edition the session looks in — one entry for the hub and
    // the mock (`Engine::build_cursor`).
    let built = engine.build_cursor(s.kind, cursor, s.edition, game);
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
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Receiver, channel, sync_channel};
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
            source_spec: None,
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
        let mut engine = Engine::new();
        let (loader, _loader_rx) = fake_loader();
        let (mut s, rx) = session(1);
        s.set_cursor(Cursor::List, wowdps_proto::EditionRef::Follow);

        push_cursor(&mut s, &mut engine, &loader, false);
        let first = rx.try_recv().expect("the first push always lands");
        let DaemonMsg::SegmentList { seq: seq1, .. } = first else {
            panic!("a List cursor is answered with SegmentList, got {first:?}");
        };

        // Two more ticks with nothing changed: dedup swallows both.
        push_cursor(&mut s, &mut engine, &loader, false);
        push_cursor(&mut s, &mut engine, &loader, false);
        assert!(
            rx.try_recv().is_err(),
            "unchanged list must not be repushed"
        );

        // The engine changes shape; the next tick pushes, seq strictly up.
        engine.on_tail(TailEvent::Lines(vec![hit(0, 0)]), &mut Vec::new());
        push_cursor(&mut s, &mut engine, &loader, false);
        let DaemonMsg::SegmentList {
            seq: seq2, entries, ..
        } = rx.try_recv().expect("a changed list is pushed")
        else {
            panic!("expected SegmentList");
        };
        assert!(seq2 > seq1, "per-session seq is monotonic");
        assert_eq!(entries.len(), 1, "the new segment is on it");
    }

    /// A `Watch` is answered inside `handle`, not on the next tick: a view
    /// change or drilldown open costs one round trip. Re-declaring the same
    /// cursor resets dedup, so the immediate reply is never suppressed by
    /// the identical snapshot pushed a moment ago.
    #[test]
    fn watch_is_answered_immediately_and_redeclaring_resets_dedup() {
        let mut engine = Engine::new();
        let (loader, _loader_rx) = fake_loader();
        let mut supervisor = Supervisor::disabled();
        let opts = hub_opts();
        let mut last_list = ListKey::default();
        let mut game = false;
        let mut shutdown = false;

        let (s, rx) = session(1);
        let mut sessions = vec![s];

        let watch = |sessions: &mut Vec<Session>,
                     engine: &mut Engine,
                     supervisor: &mut Supervisor,
                     last_list: &mut ListKey,
                     game: &mut bool,
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
                last_list,
                game,
                shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        watch(
            &mut sessions,
            &mut engine,
            &mut supervisor,
            &mut last_list,
            &mut game,
            &mut shutdown,
        );
        assert!(
            matches!(rx.try_recv(), Ok(DaemonMsg::SegmentList { .. })),
            "the reply is already queued when handle returns"
        );

        // Same cursor again: identical content, but set_cursor cleared the
        // dedup slot, so the client still gets its answer.
        watch(
            &mut sessions,
            &mut engine,
            &mut supervisor,
            &mut last_list,
            &mut game,
            &mut shutdown,
        );
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
        let mut engine = Engine::new();
        let (loader, _loader_rx) = fake_loader();
        let mut supervisor = Supervisor::disabled();
        let opts = hub_opts();
        let mut last_list = ListKey::default();
        let mut game = false;
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

        let mut tail = |sessions: &mut Vec<Session>,
                        engine: &mut Engine,
                        last_list: &mut ListKey,
                        line: String| {
            handle(
                HubMsg::Tail(TailEvent::Lines(vec![line])),
                engine,
                sessions,
                &loader,
                &mut supervisor,
                &opts,
                last_list,
                &mut game,
                &mut shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        // First line: a segment appears, the table changes shape.
        tail(&mut sessions, &mut engine, &mut last_list, hit(0, 0));
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
        tail(&mut sessions, &mut engine, &mut last_list, hit(0, 1));
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
        let mut engine = Engine::new();
        let (loader, _loader_rx) = fake_loader();
        let mut supervisor = Supervisor::disabled();
        let opts = hub_opts();
        let mut last_list = ListKey::default();
        let mut game = false;
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

        let mut fail = |sessions: &mut Vec<Session>, engine: &mut Engine| {
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
                &mut last_list,
                &mut game,
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
        engine: Engine,
        sessions: Vec<Session>,
        loader: Sender<LoadReq>,
        loader_rx: Receiver<LoadReq>,
        supervisor: Supervisor,
        opts: HubOptions,
        last_list: ListKey,
        game: bool,
        shutdown: bool,
        history: HistoryLink,
    }

    impl Hub {
        fn new() -> Self {
            let (loader, loader_rx) = fake_loader();
            Hub {
                engine: Engine::new(),
                sessions: Vec::new(),
                loader,
                loader_rx,
                supervisor: Supervisor::new(false, Duration::ZERO, None),
                opts: hub_opts(),
                last_list: ListKey::default(),
                game: false,
                shutdown: false,
                history: HistoryLink::disabled("test"),
            }
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
                &mut self.engine,
                &mut self.sessions,
                &self.loader,
                &mut self.supervisor,
                &self.opts,
                &mut self.last_list,
                &mut self.game,
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
            self.handle(HubMsg::Tail(TailEvent::Switched(FIXTURE.into())));
            self.handle(HubMsg::Tail(TailEvent::Index {
                index: Box::new(idx),
                file_age_ms: Some(0),
            }));
            self.handle(HubMsg::Tail(TailEvent::CaughtUp));
            self.engine.list_ids()
        }
    }

    fn drain(rx: &Receiver<DaemonMsg>) -> Vec<DaemonMsg> {
        let mut out = Vec::new();
        while let Ok(m) = rx.try_recv() {
            out.push(m);
        }
        out
    }

    /// v49: a log arrives as the tail delivers it — `Switched`, `Index`,
    /// then the seed lines, the version line among them — so its edition is
    /// known only after its ids are, and the seed lines need not change the
    /// ids (a log with no visit open). The list is broadcast again when the
    /// edition lands: a session pinned to the edition the log turns out to
    /// be (watching `Live`, so the broadcast is its only id table) is sent
    /// the list then, and a follower's list stops naming no edition, its
    /// rows Forever's rather than unknown.
    #[test]
    fn the_list_is_broadcast_again_once_the_seed_lines_name_the_edition() {
        use wowdps_model::Edition;
        use wowdps_proto::EditionRef;
        let dir = std::env::temp_dir().join(format!("wowdps-hub-seeds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("WoWCombatLog-072726_200000.txt");
        let text = [
            "7/27/2026 20:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18",
            "7/27/2026 20:00:10.000-4  ENCOUNTER_START,3130,\"Boss\",1,5,2769",
            "7/27/2026 20:00:11.000-4  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-9,\"Boss\",0xa48,0x0,116,\"Frostbolt\",16,900,900,0,0,0,0,0,nil,nil",
            "7/27/2026 20:00:40.000-4  ENCOUNTER_END,3130,\"Boss\",1,5,1,30000",
            "",
        ]
        .join("\n");
        std::fs::write(&path, &text).unwrap();

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
        drain(&follower);
        drain(&pinned);

        let bytes = text.as_bytes();
        let idx = wowdps_core::index::scan(&mut &bytes[..]);
        assert!(idx.open.is_none() && idx.open_visit.is_none());
        let seeds: Vec<String> = idx
            .checkpoint
            .seeds
            .iter()
            .map(|&(a, b)| {
                String::from_utf8_lossy(&bytes[a as usize..b as usize])
                    .trim_end()
                    .to_string()
            })
            .collect();
        assert!(seeds.iter().any(|l| l.contains("COMBAT_LOG_VERSION")));
        hub.handle(HubMsg::Tail(TailEvent::Switched(path.clone())));
        hub.handle(HubMsg::Tail(TailEvent::Index {
            index: Box::new(idx),
            file_age_ms: Some(0),
        }));
        assert_eq!(hub.engine.edition(), None, "the index names no edition");
        let ids = hub.engine.list_ids();
        assert!(!ids.is_empty());
        hub.handle(HubMsg::Tail(TailEvent::Seeds(seeds)));
        assert_eq!(hub.engine.edition(), Some(Edition::Forever));
        assert_eq!(hub.engine.list_ids(), ids, "the seed lines moved no id");

        let last_list = |msgs: Vec<DaemonMsg>| {
            msgs.into_iter().rev().find_map(|m| match m {
                DaemonMsg::SegmentList {
                    entries, edition, ..
                } => Some((entries, edition)),
                _ => None,
            })
        };
        let (entries, edition) = last_list(drain(&pinned)).expect("the pinned session's list");
        assert_eq!(entries.len(), ids.len());
        assert_eq!(edition, Some(Edition::Forever));
        let (entries, edition) = last_list(drain(&follower)).expect("the follower's list");
        assert_eq!(edition, Some(Edition::Forever));
        assert!(entries.iter().all(|e| e.row.edition == Edition::Forever));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// v49: one source, three sessions — following, pinned to the edition
    /// the log names, pinned to another. The first two are answered alike,
    /// every frame naming the log's edition; the third is answered the
    /// empty list of its own edition and hears nothing of this log: no
    /// fight opening, no list broadcast, no `Live` loadout.
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

        hub.handle(HubMsg::Tail(TailEvent::Switched(PathBuf::from(
            "/logs/a.txt",
        ))));
        hub.handle(HubMsg::Tail(TailEvent::CaughtUp));
        hub.handle(HubMsg::Tail(TailEvent::Lines(vec![
            "7/27/2026 21:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1".to_string(),
            hit(0, 1),
        ])));
        assert_eq!(hub.engine.edition(), Some(Edition::Retail));
        for rx in [&follower, &retail] {
            let got = drain(rx);
            assert!(
                got.iter()
                    .any(|m| matches!(m, DaemonMsg::SegmentOpened { .. })),
                "{got:?}"
            );
            assert_eq!(list_edition(&got), [(1, Some(Edition::Retail))]);
        }
        assert!(drain(&forever).is_empty(), "another edition's fight");

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

        hub.handle(HubMsg::Tail(TailEvent::Switched(PathBuf::from(
            "/logs/a.txt",
        ))));
        assert!(retired(&rx).is_empty(), "nothing precedes the first log");
        hub.handle(HubMsg::Tail(TailEvent::Switched(PathBuf::from(
            "/logs/a.txt",
        ))));
        assert!(retired(&rx).is_empty(), "the same file is not a switch");
        hub.handle(HubMsg::Tail(TailEvent::Switched(PathBuf::from(
            "/logs/b.txt",
        ))));
        assert_eq!(retired(&rx), vec![PathBuf::from("/logs/a.txt")]);
        assert_eq!(
            hub.engine.source_path.as_deref(),
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

        hub.handle(HubMsg::Game(true));
        assert!(hub.game);
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

        hub.handle(HubMsg::Game(false));
        assert_eq!(drain(&overlay), vec![DaemonMsg::SetVisible(false)]);
        hub.handle(HubMsg::Game(false));
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
            tx.send(HubMsg::Game(true)).unwrap();
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
        let bosses: Vec<SegmentId> = match hub.engine.build_list(false) {
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
        hub.handle(HubMsg::Tail(TailEvent::Lines(vec![hit(0, 0), hit(10, 0)])));
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
        hub.handle(HubMsg::Tail(TailEvent::Lines(vec![hit(0, 0)])));
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
        hub.handle(HubMsg::Tail(TailEvent::Lines(vec![hit(0, 0)])));
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
