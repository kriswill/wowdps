//! One connected client, as the hub sees it: its cursor, its outgoing
//! channel, and the dedup state that turns "rebuild every tick" into "push
//! only what changed".

use std::sync::mpsc::{SyncSender, TrySendError};

use wowdps_core::model::SegmentId;
use wowdps_proto::{ClientKind, Cursor, DaemonMsg};

/// Outgoing queue depth per client. Snapshots are droppable (a newer one is
/// always coming), so a stalled reader loses staleness, never its connection.
pub const OUTBOX: usize = 64;

pub struct Session {
    pub id: u64,
    pub kind: ClientKind,
    pub pid: u32,
    pub tx: SyncSender<DaemonMsg>,
    pub cursor: Option<Cursor>,
    /// Per-session monotonic sequence stamped on pushed snapshots.
    seq: u64,
    /// Last pushed snapshot/list, seq zeroed, for change detection.
    last_pushed: Option<DaemonMsg>,
    /// Last load failure reported for the current cursor, so a broken cursor
    /// is reported once, not at 10 Hz.
    pub last_load_error: Option<SegmentId>,
    /// Overlay only: what the client says it is currently showing.
    pub visible: bool,
    /// The outbox jammed on a control message: the client is gone in every
    /// way that matters. The hub reaps it.
    pub dead: bool,
}

impl Session {
    pub fn new(id: u64, kind: ClientKind, pid: u32, tx: SyncSender<DaemonMsg>) -> Self {
        Self {
            id,
            kind,
            pid,
            tx,
            cursor: None,
            seq: 0,
            last_pushed: None,
            last_load_error: None,
            visible: true,
            dead: false,
        }
    }

    /// The client declared a new cursor: nothing pushed for the old one is
    /// comparable any more.
    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = Some(cursor);
        self.last_pushed = None;
        self.last_load_error = None;
    }

    /// Queue a message that must not be silently dropped (acks, status,
    /// lifecycle). A full outbox here means the client stopped reading long
    /// ago — mark it dead rather than block the hub.
    pub fn push_control(&mut self, msg: DaemonMsg) {
        match self.tx.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dead = true;
            }
        }
    }

    /// Queue an unsolicited segment-list refresh: stamped so `seq` stays
    /// monotonic, but outside the cursor dedup slot (`last_pushed` belongs to
    /// whatever the cursor watches). Delivered with control semantics — a
    /// client that misses one navigates on stale ids until the next change.
    pub fn push_list(&mut self, msg: DaemonMsg) {
        debug_assert!(matches!(msg, DaemonMsg::SegmentList { .. }));
        self.seq += 1;
        match self.tx.try_send(stamp(msg, self.seq)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dead = true;
            }
        }
    }

    /// Queue a snapshot or segment list iff it differs from the last one
    /// pushed. A full outbox drops it — snapshots are idempotent and a newer
    /// one arrives next tick.
    pub fn push_snapshot(&mut self, msg: DaemonMsg) {
        debug_assert!(matches!(
            msg,
            DaemonMsg::Snapshot { .. }
                | DaemonMsg::SegmentList { .. }
                | DaemonMsg::CompareSnapshot { .. }
        ));
        if self.last_pushed.as_ref() == Some(&msg) {
            return;
        }
        self.seq += 1;
        let stamped = stamp(msg.clone(), self.seq);
        match self.tx.try_send(stamped) {
            Ok(()) => self.last_pushed = Some(msg),
            Err(TrySendError::Full(_)) => {} // stale by definition; drop
            Err(TrySendError::Disconnected(_)) => self.dead = true,
        }
    }
}

/// Rebuild a snapshot/list with its per-session sequence number. Shared with
/// the mock daemon so the two cannot drift when the message shape changes.
pub(crate) fn stamp(msg: DaemonMsg, seq: u64) -> DaemonMsg {
    match msg {
        DaemonMsg::Snapshot {
            seq: _,
            segment,
            id,
            view,
            info,
            rows,
            total_rows,
            breakdown,
            segment_count,
            source,
            status,
        } => DaemonMsg::Snapshot {
            seq,
            segment,
            id,
            view,
            info,
            rows,
            total_rows,
            breakdown,
            segment_count,
            source,
            status,
        },
        DaemonMsg::SegmentList {
            seq: _,
            entries,
            source,
            active,
        } => DaemonMsg::SegmentList {
            seq,
            entries,
            source,
            active,
        },
        // R12
        DaemonMsg::CompareSnapshot {
            seq: _,
            segment,
            id,
            info,
            a,
            b,
            range,
            source,
            status,
        } => DaemonMsg::CompareSnapshot {
            seq,
            segment,
            id,
            info,
            a,
            b,
            range,
            source,
            status,
        },
        other => other,
    }
}
