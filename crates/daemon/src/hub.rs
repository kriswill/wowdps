//! The hub thread: owns the engine and the session table, applies client
//! messages, and on each tick rebuilds snapshots for watched cursors only,
//! pushing the ones that changed. Snapshot rate is capped by the tick.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender};
use std::time::{Duration, Instant};

use wowdps_core::index::SegmentMeta;
use wowdps_core::model::{Meter, SegmentId};
use wowdps_core::tail::TailEvent;
use wowdps_proto::{
    ClientKind, ClientMsg, Cursor, DaemonMsg, HistoryAnswer, HistoryQuery, LoadError,
    PROTO_VERSION, SegmentRef,
};

use crate::engine::{Built, Engine, EngineEvent, LoadoutBuilt};
use crate::history::{HistoryLink, HistoryReq, LogRef};
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
    let mut last_ids: Vec<SegmentId> = Vec::new();
    let mut game_running = false;
    let mut shutdown = false;
    let mut idle_since: Option<Instant> = Some(Instant::now());
    let mut last_tick = Instant::now();

    while !shutdown {
        let timeout = opts.tick.saturating_sub(last_tick.elapsed());
        match rx.recv_timeout(timeout) {
            Ok(msg) => handle(
                msg,
                &mut engine,
                &mut sessions,
                &loader,
                &mut supervisor,
                &opts,
                &mut last_ids,
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

        // Idle-exit: only a watching session or the overlay supervisor
        // (live child / mid-exit-grace) holds the daemon open.
        let holding = sessions.iter().any(|s| s.cursor.is_some()) || supervisor.holds_daemon_open();
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
    // Dropping the sessions' senders lets every writer thread run down,
    // which shuts each stream and unblocks its reader.
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
    last_ids: &mut Vec<SegmentId>,
    game_running: &mut bool,
    shutdown: &mut bool,
    history: &HistoryLink,
) {
    match msg {
        HubMsg::Tail(ev) => {
            // The tailed log's index goes to the history store too: its
            // closed segments are backlog, imported off this thread.
            if history.enabled()
                && let TailEvent::Index { index, .. } = &ev
                && let Some(path) = engine.source_path.clone()
            {
                history.send(HistoryReq::Index {
                    log: LogRef { path },
                    segments: index.segments.clone(),
                    overalls: index.overalls.clone(),
                });
            }
            let mut events = Vec::new();
            engine.on_tail(ev, &mut events);
            for ev in events {
                match ev {
                    EngineEvent::Opened(id) => {
                        for s in sessions.iter_mut() {
                            s.push_control(DaemonMsg::SegmentOpened { id });
                        }
                    }
                    // One clone and a try_send: every other byte of history
                    // work happens on its own thread.
                    EngineEvent::Closed(id) => {
                        if history.enabled()
                            && let Some(fight) = engine.take_closed(id)
                        {
                            history.send(HistoryReq::Store(Box::new(fight)));
                        }
                    }
                }
            }
            // The id table changed shape: broadcast the list to every
            // session, not just List watchers. Off-list navigation resolves
            // neighbors by id, and a segment that opened *and closed* inside
            // one flush burst never gets a `SegmentOpened` of its own
            // (`Opened` only covers a batch's still-open tail).
            let ids = engine.list_ids();
            if ids != *last_ids {
                *last_ids = ids;
                let list = engine.build_list(*game_running);
                for s in sessions.iter_mut() {
                    s.push_list(list.clone());
                }
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
                ClientMsg::Watch(cursor) => {
                    s.set_cursor(cursor);
                    // Answer immediately: a view change or drilldown open is
                    // one round trip, not one round trip plus a tick.
                    push_cursor(s, engine, loader, *game_running);
                }
                ClientMsg::GetStatus { req_id } => {
                    let clients = sessions.len() as u32;
                    let source = opts.source_spec.clone();
                    let overlay = supervisor.state();
                    let history = history.status();
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
                    let ids = engine.list_ids();
                    if ids != *last_ids {
                        *last_ids = ids;
                        let list = engine.build_list(*game_running);
                        for s in sessions.iter_mut() {
                            s.push_list(list.clone());
                        }
                    }
                }
                // v19: a one-shot, answered like GetStatus when the segment
                // is warm; a cold segment parks the request behind the
                // loader and `HubMsg::Loaded` answers it.
                ClientMsg::GetLoadout {
                    req_id,
                    segment,
                    guid,
                } => match engine.loadout(segment, &guid) {
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
                // disabled store answers empty right here, never an error.
                ClientMsg::GetHistory { req_id, query } => {
                    if history.enabled() {
                        history.send(HistoryReq::Query {
                            session: id,
                            req_id,
                            query,
                        });
                    } else {
                        let answer = match query {
                            HistoryQuery::Fights { .. } => HistoryAnswer::Fights(Vec::new()),
                            HistoryQuery::Progression { .. } => HistoryAnswer::Progression {
                                pulls: 0,
                                kills: 0,
                                first_kill: None,
                                nights: Vec::new(),
                                median_kill_ms: None,
                            },
                            HistoryQuery::Trend { .. } => HistoryAnswer::Trend(Vec::new()),
                        };
                        s.push_control(DaemonMsg::History { req_id, answer });
                    }
                }
                ClientMsg::GetFight {
                    req_id,
                    fight_id,
                    view,
                    drill,
                } => {
                    if history.enabled() {
                        history.send(HistoryReq::Fight {
                            session: id,
                            req_id,
                            fight_id,
                            view,
                            drill,
                        });
                    } else {
                        s.push_control(DaemonMsg::Fight {
                            req_id,
                            fight: None,
                        });
                    }
                }
                ClientMsg::PinFight {
                    req_id,
                    fight_id,
                    pinned,
                } => {
                    if history.enabled() {
                        history.send(HistoryReq::Pin {
                            session: id,
                            req_id,
                            fight_id,
                            pinned,
                        });
                    } else {
                        s.push_control(DaemonMsg::History {
                            req_id,
                            answer: HistoryAnswer::Pinned {
                                fight_id,
                                pinned: false,
                            },
                        });
                    }
                }
                ClientMsg::ImportLog { req_id, path } => {
                    if history.enabled() {
                        history.send(HistoryReq::ImportLog {
                            session: id,
                            req_id,
                            path: std::path::PathBuf::from(path),
                        });
                    } else {
                        s.push_control(DaemonMsg::History {
                            req_id,
                            answer: HistoryAnswer::Imported { queued: 0 },
                        });
                    }
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
                    // Whoever is waiting on this segment gets it now, not at
                    // the next tick.
                    for s in sessions.iter_mut() {
                        if cursor_wants(s.cursor.as_ref(), id) {
                            push_cursor(s, engine, loader, *game_running);
                        }
                    }
                }
                Err(e) => {
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
    let Some(cursor) = s.cursor.clone() else {
        return;
    };
    let built = match cursor {
        Cursor::List => {
            s.push_snapshot(engine.build_list(game));
            return;
        }
        Cursor::Compare {
            segment,
            a,
            b,
            range,
            spell,
        } => engine.build_compare(segment, &a, &b, range, spell.as_deref()),
        Cursor::Segment {
            segment,
            view,
            top_n,
            drill,
            spell,
        } => engine.build_segment(segment, view, top_n, drill.as_deref(), spell.as_deref()),
    };
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
            spell: None,
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
        s.set_cursor(Cursor::List);

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
        let mut last_ids: Vec<SegmentId> = Vec::new();
        let mut game = false;
        let mut shutdown = false;

        let (s, rx) = session(1);
        let mut sessions = vec![s];

        let watch = |sessions: &mut Vec<Session>,
                     engine: &mut Engine,
                     supervisor: &mut Supervisor,
                     last_ids: &mut Vec<SegmentId>,
                     game: &mut bool,
                     shutdown: &mut bool| {
            handle(
                HubMsg::Client {
                    id: 1,
                    msg: ClientMsg::Watch(Cursor::List),
                },
                engine,
                sessions,
                &loader,
                supervisor,
                &opts,
                last_ids,
                game,
                shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        watch(
            &mut sessions,
            &mut engine,
            &mut supervisor,
            &mut last_ids,
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
            &mut last_ids,
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
        let mut last_ids: Vec<SegmentId> = Vec::new();
        let mut game = false;
        let mut shutdown = false;

        let (mut lister, list_rx) = session(1);
        lister.set_cursor(Cursor::List);
        let (mut watcher, watch_rx) = session(2);
        watcher.set_cursor(seg_cursor(SegmentRef::Live));
        let (idle, idle_rx) = session(3); // connected, no cursor yet
        let mut sessions = vec![lister, watcher, idle];

        let mut tail = |sessions: &mut Vec<Session>,
                        engine: &mut Engine,
                        last_ids: &mut Vec<SegmentId>,
                        line: String| {
            handle(
                HubMsg::Tail(TailEvent::Lines(vec![line])),
                engine,
                sessions,
                &loader,
                &mut supervisor,
                &opts,
                last_ids,
                &mut game,
                &mut shutdown,
                &HistoryLink::disabled("test"),
            );
        };

        // First line: a segment appears, the table changes shape.
        tail(&mut sessions, &mut engine, &mut last_ids, hit(0, 0));
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
        tail(&mut sessions, &mut engine, &mut last_ids, hit(0, 1));
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
        let mut last_ids: Vec<SegmentId> = Vec::new();
        let mut game = false;
        let mut shutdown = false;

        let wanted = SegmentId(3);
        let (mut a, a_rx) = session(1);
        a.set_cursor(seg_cursor(SegmentRef::Id(wanted)));
        let (mut b, b_rx) = session(2);
        b.set_cursor(seg_cursor(SegmentRef::Id(SegmentId(4))));
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
                &mut last_ids,
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
        last_ids: Vec<SegmentId>,
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
                last_ids: Vec::new(),
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
                &mut self.last_ids,
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

        let lines = wowdps_core::index::load_segment(&req.path, &req.meta).unwrap();
        let meter = wowdps_core::meter::meter_from_lines(lines.iter().map(String::as_str));
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
            ClientMsg::Watch(Cursor::Compare {
                segment: SegmentRef::Live,
                a: "Player-1-A".to_string(),
                b: "Player-1-B".to_string(),
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
}
