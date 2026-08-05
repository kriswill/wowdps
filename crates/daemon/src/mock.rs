//! An in-process fake daemon for client render tests: the *real* engine fed
//! the *real* fixture, answering with the *real* protocol messages —
//! synchronously, no sockets, no threads. What `testkit` was to `App`, this
//! is to `ClientState`.

use std::path::PathBuf;

use wowdps_core::index::{self, load_segment};
use wowdps_core::meter::meter_from_lines;
use wowdps_core::tail::TailEvent;
use wowdps_proto::{ClientMsg, Cursor, DaemonMsg};

use crate::engine::{Built, Engine, EngineEvent};
use crate::session::stamp;

/// The committed fixture log, resolved from this crate's source tree.
pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");

pub struct MockDaemon {
    engine: Engine,
    path: PathBuf,
    cursor: Option<Cursor>,
    seq: u64,
    game_running: bool,
    /// Pushes produced by feeds that the next `handle`/`drain` returns.
    pending: Vec<DaemonMsg>,
    /// Mirrors the hub's id-change detector for the list broadcast.
    last_ids: Vec<wowdps_core::model::SegmentId>,
}

impl MockDaemon {
    /// The whole fixture: every segment closed, indexed history.
    pub fn fixture() -> Self {
        let bytes = std::fs::read(FIXTURE).expect("fixture exists");
        Self::over(bytes)
    }

    /// The fixture cut before its final ENCOUNTER_END: the last fight is
    /// open and the daemon considers combat active (as if the game were
    /// running), like arriving mid-pull.
    pub fn fixture_live() -> Self {
        let bytes = std::fs::read(FIXTURE).expect("fixture exists");
        let text = String::from_utf8_lossy(&bytes);
        let cut = text.rfind("ENCOUNTER_END").expect("fixture has encounters");
        let mut mock = Self::over(bytes[..cut].to_vec());
        mock.game_running = true;
        mock
    }

    /// Replay `bytes` the way the tail thread would: scan, `Switched`,
    /// `Index`, the open segment's lines, `CaughtUp`.
    fn over(bytes: Vec<u8>) -> Self {
        let idx = index::scan(&mut &bytes[..]);
        let live = idx.live_offset as usize;
        let mut engine = Engine::new();
        let mut events = Vec::new();
        engine.on_tail(TailEvent::Switched(PathBuf::from(FIXTURE)), &mut events);
        engine.on_tail(
            TailEvent::Index {
                index: idx,
                file_age_ms: Some(0),
            },
            &mut events,
        );
        let tail: Vec<String> = String::from_utf8_lossy(&bytes[live..])
            .lines()
            .map(str::to_string)
            .collect();
        engine.on_tail(TailEvent::Lines(tail), &mut events);
        engine.on_tail(TailEvent::CaughtUp, &mut events);
        let last_ids = engine.list_ids();
        Self {
            engine,
            path: PathBuf::from(FIXTURE),
            cursor: None,
            seq: 0,
            game_running: false,
            pending: Vec::new(),
            last_ids,
        }
    }

    /// Process one client message, synchronously, returning every push the
    /// real daemon would eventually send for it (loads serviced inline).
    pub fn handle(&mut self, msg: ClientMsg) -> Vec<DaemonMsg> {
        let mut out = std::mem::take(&mut self.pending);
        if let ClientMsg::Watch(cursor) = msg {
            self.cursor = Some(cursor);
            self.push_cursor(&mut out);
        }
        out
    }

    /// Append live combat lines and return the resulting pushes
    /// (`SegmentOpened`, refreshed snapshots) for the current cursor.
    pub fn feed(&mut self, lines: Vec<String>) -> Vec<DaemonMsg> {
        let mut events = Vec::new();
        self.engine.on_tail(TailEvent::Lines(lines), &mut events);
        let mut out = Vec::new();
        for EngineEvent::Opened(id) in events {
            out.push(DaemonMsg::SegmentOpened { id });
        }
        // Mirror the hub: an id-table change broadcasts the list to every
        // session regardless of cursor.
        let ids = self.engine.list_ids();
        if ids != self.last_ids {
            self.last_ids = ids;
            self.seq += 1;
            out.push(stamp(self.engine.build_list(self.game_running), self.seq));
        }
        self.push_cursor(&mut out);
        out
    }

    fn push_cursor(&mut self, out: &mut Vec<DaemonMsg>) {
        let Some(cursor) = self.cursor.clone() else {
            return;
        };
        let msg = match cursor {
            Cursor::List => self.engine.build_list(self.game_running),
            Cursor::Segment {
                segment,
                view,
                top_n,
                drill,
            } => {
                // Service loads inline until the build settles.
                loop {
                    match self
                        .engine
                        .build_segment(segment, view, top_n, drill.as_deref())
                    {
                        Built::Ready(msg) => break *msg,
                        Built::Loading(_, id, meta) => {
                            let lines =
                                load_segment(&self.path, &meta).expect("fixture slice loads");
                            let meter = meter_from_lines(lines.iter().map(String::as_str));
                            self.engine.install_loaded(id, meter);
                        }
                        Built::Failed(id, error) => {
                            out.push(DaemonMsg::LoadFailed { segment: id, error });
                            return;
                        }
                    }
                }
            }
        };
        self.seq += 1;
        out.push(stamp(msg, self.seq));
    }
}

/// Drive a `ClientState` and a `MockDaemon` to quiescence: send `reqs`, feed
/// every reply back into the state, repeat until nothing more moves. This is
/// the whole client/daemon round-trip cycle, synchronously.
pub fn pump(
    state: &mut wowdps_proto::ClientState,
    mock: &mut MockDaemon,
    mut reqs: Vec<ClientMsg>,
) {
    for _ in 0..8 {
        if reqs.is_empty() {
            return;
        }
        let mut replies = Vec::new();
        for req in reqs.drain(..) {
            replies.extend(mock.handle(req));
        }
        for reply in replies {
            reqs.extend(state.on_msg(reply));
        }
    }
    panic!("client/daemon exchange should settle");
}
