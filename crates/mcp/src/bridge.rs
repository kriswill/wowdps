//! The daemon side of the MCP server: one `DaemonClient` connection reused
//! across tool calls. MCP is request/response while the daemon is
//! subscription-push, so each call declares the cursor it needs and blocks
//! (bounded) for the first snapshot that answers it — the coalescing inbox
//! already guarantees that snapshot is the freshest one.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use wowdps_model::SegmentId;
use wowdps_proto::{
    ClientKind, ClientMsg, CompareSide, Cursor, DaemonClient, DaemonMsg, ListEntry, OverlayState,
    SegmentRef,
};

/// Historical segments go through the loader pool; a cold 300 MB log segment
/// can take a few seconds. Anything beyond this is a wedged daemon.
const DEADLINE: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(10);

pub struct Bridge {
    client: DaemonClient,
    next_req: u32,
}

/// One `DaemonMsg::Snapshot`, unpacked for the tools.
pub struct Snap {
    pub id: Option<SegmentId>,
    pub info: wowdps_model::SegmentInfo,
    pub rows: Vec<wowdps_model::Row>,
    pub total_rows: u32,
    pub breakdown: Option<wowdps_proto::Breakdown>,
}

pub struct Status {
    pub game_running: bool,
    pub source: Option<String>,
    pub clients: u32,
    pub linger: bool,
    pub overlay: OverlayState,
}

impl Bridge {
    /// Connect to the daemon, spawning one on demand — the daemon binary is
    /// the `wowdps` dispatcher itself, preferred as a sibling of this binary
    /// (same build), else found on $PATH.
    pub fn connect() -> std::io::Result<Bridge> {
        let bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("wowdps")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from("wowdps"));
        let client = DaemonClient::connect(&bin, None, ClientKind::Mcp)?;
        Ok(Bridge {
            client,
            next_req: 1,
        })
    }

    /// Over an existing stream (tests).
    pub fn over(stream: std::os::unix::net::UnixStream) -> std::io::Result<Bridge> {
        let client = DaemonClient::over(stream, ClientKind::Mcp)?;
        Ok(Bridge {
            client,
            next_req: 1,
        })
    }

    /// A daemon that idle-exited between tool calls is respawned, not
    /// reported — the next answer is what the caller wants either way.
    fn revive(&mut self) {
        if self.client.is_dead() {
            self.client.reconnect_if_dead();
        }
    }

    /// Block (bounded) until `pick` claims a message.
    fn wait<T>(&mut self, mut pick: impl FnMut(DaemonMsg) -> Option<T>) -> Result<T, String> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            for msg in self.client.poll() {
                if let DaemonMsg::Fatal(e) = &msg {
                    return Err(format!("daemon: {e}"));
                }
                if let Some(t) = pick(msg) {
                    return Ok(t);
                }
            }
            if self.client.is_dead() {
                return Err("daemon connection lost".to_string());
            }
            if Instant::now() >= deadline {
                return Err("daemon did not answer in time".to_string());
            }
            std::thread::sleep(POLL);
        }
    }

    pub fn status(&mut self) -> Result<Status, String> {
        self.revive();
        let req_id = self.next_req;
        self.next_req += 1;
        self.client.send(&ClientMsg::GetStatus { req_id });
        self.wait(|msg| match msg {
            DaemonMsg::Status {
                req_id: got,
                game_running,
                source,
                clients,
                linger,
                overlay,
            } if got == req_id => Some(Status {
                game_running,
                source,
                clients,
                linger,
                overlay,
            }),
            _ => None,
        })
    }

    /// The segment list plus the daemon's liveness verdict.
    pub fn segments(&mut self) -> Result<(Vec<ListEntry>, bool, Option<String>), String> {
        self.revive();
        self.client.watch(Cursor::List);
        self.wait(|msg| match msg {
            DaemonMsg::SegmentList {
                entries,
                active,
                source,
                ..
            } => Some((entries, active, source)),
            _ => None,
        })
    }

    /// One meter snapshot for `cursor` (which must be a `Cursor::Segment`).
    pub fn snapshot(&mut self, cursor: Cursor) -> Result<Snap, String> {
        let (want_seg, want_view, want_drill) = match &cursor {
            Cursor::Segment {
                segment,
                view,
                drill,
                ..
            } => (*segment, *view, drill.is_some()),
            _ => return Err("snapshot() takes a segment cursor".to_string()),
        };
        self.revive();
        self.client.watch(cursor);
        self.wait(|msg| match msg {
            DaemonMsg::Snapshot {
                segment,
                id,
                view,
                info,
                rows,
                total_rows,
                breakdown,
                ..
            } if segment == want_seg && view == want_view && breakdown.is_some() == want_drill => {
                Some(Ok(Snap {
                    id,
                    info,
                    rows,
                    total_rows,
                    breakdown,
                }))
            }
            // Only one cursor is ever outstanding, so any load failure is ours.
            DaemonMsg::LoadFailed { error, .. } => Some(Err(load_error(error))),
            _ => None,
        })?
    }

    /// One comparison snapshot: two players of one segment, side by side.
    pub fn compare(
        &mut self,
        segment: SegmentRef,
        a: String,
        b: String,
    ) -> Result<(wowdps_model::SegmentInfo, CompareSide, CompareSide), String> {
        self.revive();
        self.client.watch(Cursor::Compare {
            segment,
            a,
            b,
            range: None,
            spell: None,
        });
        self.wait(|msg| match msg {
            DaemonMsg::CompareSnapshot {
                segment: got,
                info,
                a,
                b,
                ..
            } if got == segment => Some(Ok((info, *a, *b))),
            DaemonMsg::LoadFailed { error, .. } => Some(Err(load_error(error))),
            _ => None,
        })?
    }
}

fn load_error(e: wowdps_proto::LoadError) -> String {
    match e {
        wowdps_proto::LoadError::NotFound => {
            "no such segment (ids are per daemon run — call list_fights again)".to_string()
        }
        wowdps_proto::LoadError::Rotated => {
            "the log rotated out from under that segment".to_string()
        }
        wowdps_proto::LoadError::Io(msg) => format!("loading the segment failed: {msg}"),
    }
}
