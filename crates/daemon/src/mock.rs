//! An in-process fake daemon for client render tests: the *real* engine fed
//! the *real* fixture, answering with the *real* protocol messages —
//! synchronously, no sockets, no threads. What `testkit` was to `App`, this
//! is to `ClientState`.

use std::path::PathBuf;

use wowdps_core::index::{self, load_segment};
use wowdps_core::meter::meter_from_lines;
use wowdps_core::tail::TailEvent;
use wowdps_proto::{ClientMsg, Cursor, DaemonMsg};

use crate::engine::{Built, Engine, EngineEvent, LoadoutBuilt};
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

/// The fixture's bytes. Missing it is a broken checkout, not a runtime
/// condition — assert rather than paper over an empty log.
fn fixture_bytes() -> Vec<u8> {
    let bytes = std::fs::read(FIXTURE).ok();
    assert!(bytes.is_some(), "fixture exists: {FIXTURE}");
    bytes.unwrap_or_default()
}

impl MockDaemon {
    /// The whole fixture: every segment closed, indexed history.
    pub fn fixture() -> Self {
        Self::over(fixture_bytes())
    }

    /// The fixture cut before its final ENCOUNTER_END: the last fight is
    /// open and the daemon considers combat active (as if the game were
    /// running), like arriving mid-pull.
    pub fn fixture_live() -> Self {
        let bytes = fixture_bytes();
        let text = String::from_utf8_lossy(&bytes);
        let cut = text.rfind("ENCOUNTER_END");
        assert!(cut.is_some(), "fixture has encounters");
        let head = cut.and_then(|c| bytes.get(..c)).unwrap_or(&bytes);
        let mut mock = Self::over(head.to_vec());
        mock.game_running = true;
        mock
    }

    /// Replay `bytes` the way the tail thread would: scan, `Switched`,
    /// `Index`, the seed lines, the open segment's lines, `CaughtUp`.
    fn over(bytes: Vec<u8>) -> Self {
        let idx = index::scan(&mut &bytes[..]);
        let live = idx.live_offset as usize;
        // Mirror `tail.rs`: state-carrying seed lines replay into the live
        // meter before the tail, so pets, classes and visit context resolve.
        let seed_ranges = match idx.open.as_ref() {
            Some(open) => open.seeds.clone(),
            None => idx.checkpoint.seeds.clone(),
        };
        let seeds: Vec<String> = seed_ranges
            .iter()
            .filter_map(|&(s, e)| {
                let slice = bytes.get(s as usize..e as usize)?;
                Some(String::from_utf8_lossy(slice).trim_end().to_string())
            })
            .collect();
        let mut engine = Engine::new();
        let mut events = Vec::new();
        engine.on_tail(TailEvent::Switched(PathBuf::from(FIXTURE)), &mut events);
        engine.on_tail(
            TailEvent::Index {
                index: Box::new(idx),
                file_age_ms: Some(0),
            },
            &mut events,
        );
        if !seeds.is_empty() {
            engine.on_tail(TailEvent::Lines(seeds), &mut events);
        }
        let tail: Vec<String> = String::from_utf8_lossy(bytes.get(live..).unwrap_or_default())
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
        match msg {
            ClientMsg::Watch(cursor) => {
                self.cursor = Some(cursor);
                self.push_cursor(&mut out);
            }
            // v19: loads serviced inline, like the snapshot paths.
            ClientMsg::GetLoadout {
                req_id,
                segment,
                guid,
            } => {
                let loadout = loop {
                    match self.engine.loadout(segment, &guid) {
                        LoadoutBuilt::Ready(l) => break l,
                        LoadoutBuilt::Loading(id, meta) => {
                            let lines = load_segment(&self.path, &meta);
                            assert!(lines.is_ok(), "fixture slice loads");
                            let lines = lines.unwrap_or_default();
                            let meter = meter_from_lines(lines.iter().map(String::as_str));
                            self.engine.install_loaded(id, meter);
                        }
                    }
                };
                out.push(DaemonMsg::Loadout {
                    req_id,
                    guid,
                    loadout,
                });
            }
            _ => {}
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
        // Service loads inline until the build settles.
        macro_rules! settle {
            ($build:expr) => {
                loop {
                    match $build {
                        Built::Ready(msg) => break *msg,
                        Built::Loading(_, id, meta) => {
                            let lines = load_segment(&self.path, &meta);
                            assert!(lines.is_ok(), "fixture slice loads");
                            let lines = lines.unwrap_or_default();
                            let meter = meter_from_lines(lines.iter().map(String::as_str));
                            self.engine.install_loaded(id, meter);
                        }
                        Built::Failed(id, error) => {
                            out.push(DaemonMsg::LoadFailed { segment: id, error });
                            return;
                        }
                    }
                }
            };
        }
        let msg = match cursor {
            Cursor::List => self.engine.build_list(self.game_running),
            Cursor::Segment {
                segment,
                view,
                top_n,
                drill,
                spell,
            } => settle!(self.engine.build_segment(
                segment,
                view,
                top_n,
                drill.as_deref(),
                spell.as_deref()
            )),
            // R12
            Cursor::Compare {
                segment,
                a,
                b,
                range,
                spell,
            } => {
                settle!(
                    self.engine
                        .build_compare(segment, &a, &b, range, spell.as_deref())
                )
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
    assert!(reqs.is_empty(), "client/daemon exchange should settle");
}
