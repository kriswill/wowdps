//! One connected client, as the hub sees it: its cursor, its outgoing
//! channel, and the dedup state that turns "rebuild every tick" into "push
//! only what changed".

use std::sync::mpsc::{SyncSender, TrySendError};

use wowdps_core::model::SegmentId;
use wowdps_model::Edition;
use wowdps_proto::{ClientKind, Cursor, DaemonMsg, EditionRef};

/// Outgoing queue depth per client. Snapshots are droppable (a newer one is
/// always coming), so a stalled reader loses staleness, never its connection.
pub const OUTBOX: usize = 64;

pub struct Session {
    pub id: u64,
    pub kind: ClientKind,
    pub pid: u32,
    pub tx: SyncSender<DaemonMsg>,
    pub cursor: Option<Cursor>,
    /// v49: the edition the cursor looks in, from the last Watch (following
    /// before any): what `Live`, the list, the list broadcast and a
    /// one-shot's `Live` resolve inside.
    pub edition: EditionRef,
    /// v49: the followed source whose list the session was last sent (by a
    /// broadcast, or answering its list or `Live`), `None` before any:
    /// when another source answers its edition now, the hub sends it that
    /// one's list (`hub::resync`), which moves the client.
    pub answered_by: Option<usize>,
    /// v49: the last segment list queued to the session since its last
    /// Watch (by a broadcast or its cursor), when it had no entries
    /// ([`EmptyList`]): `hub::resync` sends no empty list the session holds
    /// already — an unserved pin's, on every live move. A list with entries
    /// leaves nothing here, so it is always sent.
    empty_list: Option<EmptyList>,
    /// Per-session monotonic sequence stamped on pushed snapshots.
    seq: u64,
    /// Last pushed snapshot/list, seq zeroed, for change detection.
    last_pushed: Option<DaemonMsg>,
    /// Last load failure reported for the current cursor, so a broken cursor
    /// is reported once, not at 10 Hz.
    pub last_load_error: Option<SegmentId>,
    /// Overlay only: what the client says it is currently showing.
    pub visible: bool,
    /// v19: `GetLoadout` requests parked behind a segment load. A one-shot
    /// is always answered, so the hub drains matching entries the moment the
    /// loader delivers — success or not.
    pub pending_loadouts: Vec<(u32, SegmentId, String)>,
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
            edition: EditionRef::Follow,
            answered_by: None,
            empty_list: None,
            seq: 0,
            last_pushed: None,
            last_load_error: None,
            visible: true,
            pending_loadouts: Vec::new(),
            dead: false,
        }
    }

    /// The client declared a new cursor (v49: in an edition): nothing pushed
    /// for the old one is comparable any more.
    pub fn set_cursor(&mut self, cursor: Cursor, edition: EditionRef) {
        self.cursor = Some(cursor);
        self.edition = edition;
        self.last_pushed = None;
        self.last_load_error = None;
        // A new Watch (another edition, say) may take the client off the
        // list it held: the next list is sent, held or not.
        self.empty_list = None;
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
        let empty = EmptyList::of(&msg);
        match self.tx.try_send(stamp(msg, self.seq)) {
            Ok(()) => self.empty_list = empty,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dead = true;
            }
        }
    }

    /// v49: whether `msg` is an empty list the session holds already: the
    /// last list queued since its Watch, entry-less and saying the same.
    pub fn holds_empty_list(&self, msg: &DaemonMsg) -> bool {
        EmptyList::of(msg).is_some_and(|e| self.empty_list.as_ref() == Some(&e))
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
            Ok(()) => {
                if matches!(msg, DaemonMsg::SegmentList { .. }) {
                    self.empty_list = EmptyList::of(&msg);
                }
                self.last_pushed = Some(msg);
            }
            Err(TrySendError::Full(_)) => {} // stale by definition; drop
            Err(TrySendError::Disconnected(_)) => self.dead = true,
        }
    }
}

/// v49: everything an entry-less segment list says but its `seq`, its log's
/// name as a 64-bit FNV-1a: what tells two empty lists apart without
/// keeping either. Only an entry-less list has one; two with the same are
/// the same list but for a hash collision of two log names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EmptyList {
    source: Option<u64>,
    active: bool,
    log_id: Option<u64>,
    edition: Option<Edition>,
}

impl EmptyList {
    fn of(msg: &DaemonMsg) -> Option<Self> {
        match msg {
            DaemonMsg::SegmentList {
                entries,
                source,
                active,
                log_id,
                edition,
                ..
            } if entries.is_empty() => Some(Self {
                source: source.as_deref().map(|s| {
                    s.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
                    })
                }),
                active: *active,
                log_id: *log_id,
                edition: *edition,
            }),
            _ => None,
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
            raid,
            edition,
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
            raid,
            edition,
        },
        DaemonMsg::SegmentList {
            seq: _,
            entries,
            source,
            active,
            log_id,
            edition,
        } => DaemonMsg::SegmentList {
            seq,
            entries,
            source,
            active,
            log_id,
            edition,
        },
        // R12
        DaemonMsg::CompareSnapshot {
            seq: _,
            segment,
            id,
            info,
            view,
            a,
            b,
            range,
            source,
            status,
            edition,
        } => DaemonMsg::CompareSnapshot {
            seq,
            segment,
            id,
            info,
            view,
            a,
            b,
            range,
            source,
            status,
            edition,
        },
        other => other,
    }
}
