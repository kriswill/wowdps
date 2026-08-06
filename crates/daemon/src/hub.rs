//! The hub thread: owns the engine and the session table, applies client
//! messages, and on each tick rebuilds snapshots for watched cursors only,
//! pushing the ones that changed. Snapshot rate is capped by the tick.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender};
use std::time::{Duration, Instant};

use wowdps_core::model::{Meter, SegmentId};
use wowdps_core::tail::TailEvent;
use wowdps_proto::{ClientKind, ClientMsg, Cursor, DaemonMsg, LoadError, PROTO_VERSION};

use crate::engine::{Built, Engine, EngineEvent};
use crate::loader::LoadReq;
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
        result: Result<Meter, String>,
    },
    Game(bool),
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
) {
    match msg {
        HubMsg::Tail(ev) => {
            let mut events = Vec::new();
            engine.on_tail(ev, &mut events);
            for EngineEvent::Opened(id) in events {
                for s in sessions.iter_mut() {
                    s.push_control(DaemonMsg::SegmentOpened { id });
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
                ClientMsg::Hello { .. } | ClientMsg::Shutdown => {}
            }
        }
        HubMsg::Disconnected { id } => {
            supervisor.on_overlay_disconnected(id);
            sessions.retain(|s| s.id != id);
        }
        HubMsg::Loaded { id, result } => {
            engine.loading.remove(&id);
            match result {
                Ok(meter) => {
                    engine.install_loaded(id, meter);
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
        }
        HubMsg::Game(g) => {
            *game_running = g;
            let cmds = supervisor.on_game(g);
            deliver(sessions, cmds);
        }
    }
}

fn cursor_wants(cursor: Option<&Cursor>, id: SegmentId) -> bool {
    matches!(
        cursor,
        Some(Cursor::Segment {
            segment: wowdps_proto::SegmentRef::Id(i),
            ..
        }) if *i == id
    )
}

/// Build and (dedup-)push whatever `s` is watching.
fn push_cursor(s: &mut Session, engine: &mut Engine, loader: &Sender<LoadReq>, game: bool) {
    let Some(cursor) = s.cursor.clone() else {
        return;
    };
    match cursor {
        Cursor::List => s.push_snapshot(engine.build_list(game)),
        Cursor::Segment {
            segment,
            view,
            top_n,
            drill,
        } => match engine.build_segment(segment, view, top_n, drill.as_deref()) {
            Built::Ready(msg) => s.push_snapshot(*msg),
            Built::Loading(msg, id, meta) => {
                if !engine.loading.contains(&id)
                    && let Some(path) = engine.source_path.clone()
                {
                    engine.loading.insert(id);
                    let _ = loader.send(LoadReq { id, path, meta });
                }
                s.push_snapshot(*msg);
            }
            Built::Failed(id, error) => {
                if s.last_load_error != Some(id) {
                    s.last_load_error = Some(id);
                    s.push_control(DaemonMsg::LoadFailed { segment: id, error });
                }
            }
        },
    }
}
