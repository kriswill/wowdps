//! The messages. Every variant encodes to one frame (`wire::frame`) and
//! decodes from `(tag, body)`. Changing the shape of anything here — fields,
//! order, enum codes — is a `PROTO_VERSION` bump; the golden-bytes tests
//! exist to make that impossible to do by accident.

use wowdps_model::{Class, ListRow, Row, SegmentId, SegmentInfo, SegmentKind, View};

use crate::wire::{self, DecodeError, Reader, Result};

/// Version of the whole wire surface. Embedded in the socket path, so a
/// mismatch is structurally impossible rather than diagnosed at handshake.
pub const PROTO_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Tui,
    Window,
    Overlay,
    Mcp,
}

/// A segment as a client refers to it: the live one, or a stable id. Ids are
/// monotonic for the daemon's lifetime and never reused, so a stale id can
/// only fail to load — never resolve to another file's fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentRef {
    Live,
    Id(SegmentId),
}

/// What a client is rendering; the daemon pushes snapshots for exactly this.
#[derive(Debug, Clone, PartialEq)]
pub enum Cursor {
    /// The segment-list screen: pushed `SegmentList` snapshots.
    List,
    /// A segment's meter. `drill` set means snapshots carry the breakdown
    /// too, so a drilldown open on a live fight keeps updating.
    Segment {
        segment: SegmentRef,
        view: View,
        top_n: Option<u32>,
        drill: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientMsg {
    Hello {
        proto: u16,
        client: ClientKind,
        pid: u32,
    },
    /// The client's cursor: replaces any prior Watch.
    Watch(Cursor),
    GetStatus {
        req_id: u32,
    },
    /// Overlay only: the user hid/showed it locally; the supervisor agrees.
    VisibilityChanged {
        visible: bool,
    },
    /// `wowdps --stop`. Accepted pre-handshake so `--stop` always works.
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Breakdown {
    pub by_spell: Vec<Row>,
    pub by_target: Vec<Row>,
}

/// One segment-list row plus the stable id a client uses to watch it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEntry {
    pub id: SegmentId,
    pub row: ListRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    NotFound,
    Rotated,
    Io(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayState {
    /// Not running and not wanted (no game, or auto_overlay off).
    Absent,
    Visible,
    Hidden,
    /// Spawn failed; carries the child's retained stderr.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DaemonMsg {
    HelloAck {
        proto: u16,
        version: String,
    },
    Snapshot {
        seq: u64,
        /// Echoes the cursor this answers.
        segment: SegmentRef,
        /// The stable id of the segment this snapshot describes — set even
        /// when the cursor said `Live`, so a client always knows where it is
        /// in the list. `None` only when there is nothing to describe.
        id: Option<SegmentId>,
        view: View,
        info: SegmentInfo,
        rows: Vec<Row>,
        /// Greater than `rows.len()` when `top_n` truncated.
        total_rows: u32,
        /// Present iff the watched cursor drills into an actor.
        breakdown: Option<Breakdown>,
        segment_count: u32,
        /// Log file name, for the header. A change means the log rotated:
        /// clients drop cached state and re-Watch.
        source: Option<String>,
        /// Daemon-side error/notice for the footer.
        status: Option<String>,
    },
    /// Pushed to `Cursor::List` watchers: the full list, oldest first,
    /// coalesced by `seq` like snapshots. Each row carries its stable id —
    /// that id is what "open this row" turns into a `Cursor::Segment`.
    SegmentList {
        seq: u64,
        entries: Vec<ListEntry>,
        source: Option<String>,
        /// The daemon's liveness verdict: a fight is happening *now* (open
        /// segment + fresh lines or the game process running). This is what
        /// lets a freshly started client jump straight to the live meter —
        /// and what a stale log's forever-open trailing segment must not
        /// fake, which mtime heuristics got wrong during flush bursts.
        active: bool,
    },
    /// A new segment just opened on live combat — the client decides whether
    /// to snap to it.
    SegmentOpened {
        id: SegmentId,
    },
    LoadFailed {
        segment: SegmentId,
        error: LoadError,
    },
    Status {
        req_id: u32,
        game_running: bool,
        source: Option<String>,
        clients: u32,
        linger: bool,
        overlay: OverlayState,
    },
    /// Overlay lifecycle command from the supervisor.
    SetVisible(bool),
    Fatal(String),
}

// ---- enum codes -------------------------------------------------------------

fn view_code(v: View) -> u8 {
    v.index() as u8
}

fn view_from(b: u8) -> Result<View> {
    Ok(match b {
        0 => View::Damage,
        1 => View::Healing,
        2 => View::Interrupts,
        3 => View::CrowdControl,
        4 => View::Dispels,
        5 => View::Deaths,
        _ => return Err(DecodeError::BadTag(b)),
    })
}

fn kind_code(k: SegmentKind) -> u8 {
    match k {
        SegmentKind::Encounter => 0,
        SegmentKind::Trash => 1,
    }
}

fn kind_from(b: u8) -> Result<SegmentKind> {
    Ok(match b {
        0 => SegmentKind::Encounter,
        1 => SegmentKind::Trash,
        _ => return Err(DecodeError::BadTag(b)),
    })
}

fn class_code(c: Class) -> u8 {
    match c {
        Class::Warrior => 0,
        Class::Paladin => 1,
        Class::Hunter => 2,
        Class::Rogue => 3,
        Class::Priest => 4,
        Class::DeathKnight => 5,
        Class::Shaman => 6,
        Class::Mage => 7,
        Class::Warlock => 8,
        Class::Monk => 9,
        Class::Druid => 10,
        Class::DemonHunter => 11,
        Class::Evoker => 12,
    }
}

fn class_from(b: u8) -> Result<Class> {
    Ok(match b {
        0 => Class::Warrior,
        1 => Class::Paladin,
        2 => Class::Hunter,
        3 => Class::Rogue,
        4 => Class::Priest,
        5 => Class::DeathKnight,
        6 => Class::Shaman,
        7 => Class::Mage,
        8 => Class::Warlock,
        9 => Class::Monk,
        10 => Class::Druid,
        11 => Class::DemonHunter,
        12 => Class::Evoker,
        _ => return Err(DecodeError::BadTag(b)),
    })
}

fn client_kind_code(k: ClientKind) -> u8 {
    match k {
        ClientKind::Tui => 0,
        ClientKind::Window => 1,
        ClientKind::Overlay => 2,
        ClientKind::Mcp => 3,
    }
}

fn client_kind_from(b: u8) -> Result<ClientKind> {
    Ok(match b {
        0 => ClientKind::Tui,
        1 => ClientKind::Window,
        2 => ClientKind::Overlay,
        3 => ClientKind::Mcp,
        _ => return Err(DecodeError::BadTag(b)),
    })
}

// ---- composite pieces -------------------------------------------------------

fn put_segment_ref(buf: &mut Vec<u8>, r: SegmentRef) {
    match r {
        SegmentRef::Live => wire::put_u8(buf, 0),
        SegmentRef::Id(id) => {
            wire::put_u8(buf, 1);
            wire::put_u64(buf, id.0);
        }
    }
}

fn get_segment_ref(rd: &mut Reader) -> Result<SegmentRef> {
    match rd.u8()? {
        0 => Ok(SegmentRef::Live),
        1 => Ok(SegmentRef::Id(SegmentId(rd.u64()?))),
        b => Err(DecodeError::BadTag(b)),
    }
}

fn put_row(buf: &mut Vec<u8>, r: &Row) {
    wire::put_str(buf, &r.key);
    wire::put_str(buf, &r.label);
    wire::put_u64(buf, r.amount);
    wire::put_u64(buf, r.extra);
    wire::put_f64(buf, r.per_sec);
    wire::put_f64(buf, r.pct);
    wire::put_opt(buf, r.class.as_ref(), |b, c| {
        wire::put_u8(b, class_code(*c))
    });
}

fn get_row(rd: &mut Reader) -> Result<Row> {
    Ok(Row {
        key: rd.string()?,
        label: rd.string()?,
        amount: rd.u64()?,
        extra: rd.u64()?,
        per_sec: rd.f64()?,
        pct: rd.f64()?,
        class: rd.opt(|r| class_from(r.u8()?))?,
    })
}

fn put_info(buf: &mut Vec<u8>, i: &SegmentInfo) {
    wire::put_u8(buf, kind_code(i.kind));
    wire::put_str(buf, &i.name);
    wire::put_i64(buf, i.start_ms);
    wire::put_i64(buf, i.duration_ms);
    wire::put_opt(buf, i.success.as_ref(), |b, s| wire::put_bool(b, *s));
    wire::put_bool(buf, i.live);
}

fn get_info(rd: &mut Reader) -> Result<SegmentInfo> {
    Ok(SegmentInfo {
        kind: kind_from(rd.u8()?)?,
        name: rd.string()?,
        start_ms: rd.i64()?,
        duration_ms: rd.i64()?,
        success: rd.opt(|r| r.bool())?,
        live: rd.bool()?,
    })
}

fn put_list_entry(buf: &mut Vec<u8>, e: &ListEntry) {
    wire::put_u64(buf, e.id.0);
    put_list_row(buf, &e.row);
}

fn get_list_entry(rd: &mut Reader) -> Result<ListEntry> {
    Ok(ListEntry {
        id: SegmentId(rd.u64()?),
        row: get_list_row(rd)?,
    })
}

fn put_list_row(buf: &mut Vec<u8>, r: &ListRow) {
    wire::put_u8(buf, kind_code(r.kind));
    wire::put_str(buf, &r.name);
    wire::put_i64(buf, r.start_ms);
    wire::put_opt(buf, r.success.as_ref(), |b, s| wire::put_bool(b, *s));
    wire::put_i64(buf, r.duration_ms);
    wire::put_bool(buf, r.live);
}

fn get_list_row(rd: &mut Reader) -> Result<ListRow> {
    Ok(ListRow {
        kind: kind_from(rd.u8()?)?,
        name: rd.string()?,
        start_ms: rd.i64()?,
        success: rd.opt(|r| r.bool())?,
        duration_ms: rd.i64()?,
        live: rd.bool()?,
    })
}

fn put_cursor(buf: &mut Vec<u8>, c: &Cursor) {
    match c {
        Cursor::List => wire::put_u8(buf, 0),
        Cursor::Segment {
            segment,
            view,
            top_n,
            drill,
        } => {
            wire::put_u8(buf, 1);
            put_segment_ref(buf, *segment);
            wire::put_u8(buf, view_code(*view));
            wire::put_opt(buf, top_n.as_ref(), |b, n| wire::put_u32(b, *n));
            wire::put_opt(buf, drill.as_ref(), |b, d| wire::put_str(b, d));
        }
    }
}

fn get_cursor(rd: &mut Reader) -> Result<Cursor> {
    match rd.u8()? {
        0 => Ok(Cursor::List),
        1 => Ok(Cursor::Segment {
            segment: get_segment_ref(rd)?,
            view: view_from(rd.u8()?)?,
            top_n: rd.opt(|r| r.u32())?,
            drill: rd.opt(|r| r.string())?,
        }),
        b => Err(DecodeError::BadTag(b)),
    }
}

fn put_breakdown(buf: &mut Vec<u8>, b: &Breakdown) {
    wire::put_vec(buf, &b.by_spell, put_row);
    wire::put_vec(buf, &b.by_target, put_row);
}

fn get_breakdown(rd: &mut Reader) -> Result<Breakdown> {
    Ok(Breakdown {
        by_spell: rd.vec(get_row)?,
        by_target: rd.vec(get_row)?,
    })
}

fn put_load_error(buf: &mut Vec<u8>, e: &LoadError) {
    match e {
        LoadError::NotFound => wire::put_u8(buf, 0),
        LoadError::Rotated => wire::put_u8(buf, 1),
        LoadError::Io(msg) => {
            wire::put_u8(buf, 2);
            wire::put_str(buf, msg);
        }
    }
}

fn get_load_error(rd: &mut Reader) -> Result<LoadError> {
    match rd.u8()? {
        0 => Ok(LoadError::NotFound),
        1 => Ok(LoadError::Rotated),
        2 => Ok(LoadError::Io(rd.string()?)),
        b => Err(DecodeError::BadTag(b)),
    }
}

fn put_overlay_state(buf: &mut Vec<u8>, s: &OverlayState) {
    match s {
        OverlayState::Absent => wire::put_u8(buf, 0),
        OverlayState::Visible => wire::put_u8(buf, 1),
        OverlayState::Hidden => wire::put_u8(buf, 2),
        OverlayState::Failed(msg) => {
            wire::put_u8(buf, 3);
            wire::put_str(buf, msg);
        }
    }
}

fn get_overlay_state(rd: &mut Reader) -> Result<OverlayState> {
    match rd.u8()? {
        0 => Ok(OverlayState::Absent),
        1 => Ok(OverlayState::Visible),
        2 => Ok(OverlayState::Hidden),
        3 => Ok(OverlayState::Failed(rd.string()?)),
        b => Err(DecodeError::BadTag(b)),
    }
}

// ---- ClientMsg --------------------------------------------------------------

const T_HELLO: u8 = 0x01;
const T_WATCH: u8 = 0x02;
const T_GET_STATUS: u8 = 0x03;
const T_VISIBILITY: u8 = 0x04;
const T_SHUTDOWN: u8 = 0x05;

impl ClientMsg {
    /// One complete on-the-wire frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        let tag = match self {
            ClientMsg::Hello { proto, client, pid } => {
                wire::put_u16(&mut body, *proto);
                wire::put_u8(&mut body, client_kind_code(*client));
                wire::put_u32(&mut body, *pid);
                T_HELLO
            }
            ClientMsg::Watch(cursor) => {
                put_cursor(&mut body, cursor);
                T_WATCH
            }
            ClientMsg::GetStatus { req_id } => {
                wire::put_u32(&mut body, *req_id);
                T_GET_STATUS
            }
            ClientMsg::VisibilityChanged { visible } => {
                wire::put_bool(&mut body, *visible);
                T_VISIBILITY
            }
            ClientMsg::Shutdown => T_SHUTDOWN,
        };
        wire::frame(tag, &body)
    }

    pub fn decode(tag: u8, body: &[u8]) -> Result<Self> {
        let mut rd = Reader::new(body);
        let msg = match tag {
            T_HELLO => ClientMsg::Hello {
                proto: rd.u16()?,
                client: client_kind_from(rd.u8()?)?,
                pid: rd.u32()?,
            },
            T_WATCH => ClientMsg::Watch(get_cursor(&mut rd)?),
            T_GET_STATUS => ClientMsg::GetStatus { req_id: rd.u32()? },
            T_VISIBILITY => ClientMsg::VisibilityChanged {
                visible: rd.bool()?,
            },
            T_SHUTDOWN => ClientMsg::Shutdown,
            other => return Err(DecodeError::BadTag(other)),
        };
        rd.finish()?;
        Ok(msg)
    }
}

// ---- DaemonMsg --------------------------------------------------------------

const T_HELLO_ACK: u8 = 0x81;
const T_SNAPSHOT: u8 = 0x82;
const T_SEGMENT_LIST: u8 = 0x83;
const T_SEGMENT_OPENED: u8 = 0x84;
const T_LOAD_FAILED: u8 = 0x85;
const T_STATUS: u8 = 0x86;
const T_SET_VISIBLE: u8 = 0x87;
const T_FATAL: u8 = 0x88;

impl DaemonMsg {
    /// One complete on-the-wire frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        let tag = match self {
            DaemonMsg::HelloAck { proto, version } => {
                wire::put_u16(&mut body, *proto);
                wire::put_str(&mut body, version);
                T_HELLO_ACK
            }
            DaemonMsg::Snapshot {
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
            } => {
                wire::put_u64(&mut body, *seq);
                put_segment_ref(&mut body, *segment);
                wire::put_opt(&mut body, id.as_ref(), |b, i| wire::put_u64(b, i.0));
                wire::put_u8(&mut body, view_code(*view));
                put_info(&mut body, info);
                wire::put_vec(&mut body, rows, put_row);
                wire::put_u32(&mut body, *total_rows);
                wire::put_opt(&mut body, breakdown.as_ref(), put_breakdown);
                wire::put_u32(&mut body, *segment_count);
                wire::put_opt(&mut body, source.as_ref(), |b, s| wire::put_str(b, s));
                wire::put_opt(&mut body, status.as_ref(), |b, s| wire::put_str(b, s));
                T_SNAPSHOT
            }
            DaemonMsg::SegmentList {
                seq,
                entries,
                source,
                active,
            } => {
                wire::put_u64(&mut body, *seq);
                wire::put_vec(&mut body, entries, put_list_entry);
                wire::put_opt(&mut body, source.as_ref(), |b, s| wire::put_str(b, s));
                wire::put_bool(&mut body, *active);
                T_SEGMENT_LIST
            }
            DaemonMsg::SegmentOpened { id } => {
                wire::put_u64(&mut body, id.0);
                T_SEGMENT_OPENED
            }
            DaemonMsg::LoadFailed { segment, error } => {
                wire::put_u64(&mut body, segment.0);
                put_load_error(&mut body, error);
                T_LOAD_FAILED
            }
            DaemonMsg::Status {
                req_id,
                game_running,
                source,
                clients,
                linger,
                overlay,
            } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_bool(&mut body, *game_running);
                wire::put_opt(&mut body, source.as_ref(), |b, s| wire::put_str(b, s));
                wire::put_u32(&mut body, *clients);
                wire::put_bool(&mut body, *linger);
                put_overlay_state(&mut body, overlay);
                T_STATUS
            }
            DaemonMsg::SetVisible(v) => {
                wire::put_bool(&mut body, *v);
                T_SET_VISIBLE
            }
            DaemonMsg::Fatal(msg) => {
                wire::put_str(&mut body, msg);
                T_FATAL
            }
        };
        wire::frame(tag, &body)
    }

    pub fn decode(tag: u8, body: &[u8]) -> Result<Self> {
        let mut rd = Reader::new(body);
        let msg = match tag {
            T_HELLO_ACK => DaemonMsg::HelloAck {
                proto: rd.u16()?,
                version: rd.string()?,
            },
            T_SNAPSHOT => DaemonMsg::Snapshot {
                seq: rd.u64()?,
                segment: get_segment_ref(&mut rd)?,
                id: rd.opt(|r| Ok(SegmentId(r.u64()?)))?,
                view: view_from(rd.u8()?)?,
                info: get_info(&mut rd)?,
                rows: rd.vec(get_row)?,
                total_rows: rd.u32()?,
                breakdown: rd.opt(get_breakdown)?,
                segment_count: rd.u32()?,
                source: rd.opt(|r| r.string())?,
                status: rd.opt(|r| r.string())?,
            },
            T_SEGMENT_LIST => DaemonMsg::SegmentList {
                seq: rd.u64()?,
                entries: rd.vec(get_list_entry)?,
                source: rd.opt(|r| r.string())?,
                active: rd.bool()?,
            },
            T_SEGMENT_OPENED => DaemonMsg::SegmentOpened {
                id: SegmentId(rd.u64()?),
            },
            T_LOAD_FAILED => DaemonMsg::LoadFailed {
                segment: SegmentId(rd.u64()?),
                error: get_load_error(&mut rd)?,
            },
            T_STATUS => DaemonMsg::Status {
                req_id: rd.u32()?,
                game_running: rd.bool()?,
                source: rd.opt(|r| r.string())?,
                clients: rd.u32()?,
                linger: rd.bool()?,
                overlay: get_overlay_state(&mut rd)?,
            },
            T_SET_VISIBLE => DaemonMsg::SetVisible(rd.bool()?),
            T_FATAL => DaemonMsg::Fatal(rd.string()?),
            other => return Err(DecodeError::BadTag(other)),
        };
        rd.finish()?;
        Ok(msg)
    }
}
