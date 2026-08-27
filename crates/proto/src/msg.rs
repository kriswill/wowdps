//! The messages. Every variant encodes to one frame (`wire::frame`) and
//! decodes from `(tag, body)`. Changing the shape of anything here — fields,
//! order, enum codes — is a `PROTO_VERSION` bump; the golden-bytes tests
//! exist to make that impossible to do by accident.

use wowdps_model::{
    Class, ListRow, Mark, MarkKind, Row, SegmentId, SegmentInfo, SegmentKind, Spec, Timeline, View,
};

use crate::wire::{self, DecodeError, Reader, Result};

/// Version of the whole wire surface. Embedded in the socket path, so a
/// mismatch is structurally impossible rather than diagnosed at handshake.
pub const PROTO_VERSION: u16 = 18;

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
        /// v16: the second drill level — one ability of the drilled player,
        /// by its by-spell row key ("spell" or "spell\0pet"). Snapshots then
        /// carry that spell's own timeline in the breakdown. Meaningless
        /// without `drill`.
        spell: Option<String>,
    },
    /// R12: two players of one segment, side by side. Answered with
    /// `CompareSnapshot` instead of `Snapshot` — a comparison carries per-spell
    /// tables and two timelines, which no meter snapshot has a place for.
    /// The daemon holds the pair in the order given, so the panes never swap
    /// under the user when one of them overtakes the other.
    Compare {
        segment: SegmentRef,
        a: String,
        b: String,
        /// v12: window the spell tables and totals to `lo..hi` ms relative to
        /// the segment's start. `None` is the whole fight. The timelines are
        /// always sent whole — the graph zoom is the client's own slice.
        range: Option<(u32, u32)>,
        /// v18: the comparison's ability drill — ONE by-spell key applied to
        /// BOTH sides (same-class pairs share their kit), so each side's
        /// snapshot carries that spell's own curve. A side without the spell
        /// simply gets none.
        spell: Option<String>,
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
    /// R11: drop closed, out-of-instance Trash from the daemon's list — the
    /// live segment and every visit member (keys, raids: their Σ needs them)
    /// survive. Daemon-lifetime tombstones; a restart rescans everything.
    DiscardTrash,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Breakdown {
    pub by_spell: Vec<Row>,
    pub by_target: Vec<Row>,
    /// v14 (R12): the drilled player's damage timeline — the same whole-fight
    /// grid a `CompareSide` carries, so a drilldown can draw the comparison's
    /// graph without opening a second cursor. Present iff the drilled view is
    /// Damage (the buckets ARE damage; sending them under Healing would lie).
    /// v14 amendment: also present for Healing, carrying effective healing.
    pub timeline: Option<Timeline>,
    /// v16: the drilled ABILITY's own curve (`Segment::spell_timeline`),
    /// present iff the cursor names a spell and the view is Damage — drawn
    /// over the player's `timeline` ghosted behind it.
    pub spell_timeline: Option<Timeline>,
    /// v17: who the drilled ability landed on (`Segment::spell_targets`) —
    /// sorted desc, pct of the spell's own total, rows wearing the spell's
    /// school. Present iff the cursor names a spell.
    pub spell_targets: Option<Vec<Row>>,
}

/// R12: one player's half of a comparison.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CompareSide {
    /// The player's guid — the same key a meter row carries, so a client can
    /// match a side back to the row the user clicked.
    pub guid: String,
    /// The player's meter row for the segment: total, DPS, share, class and
    /// spec all arrive in the shape every renderer already knows.
    pub total: Row,
    /// Per-spell damage rows: `count` is hits, `crits` feeds `crit_pct()`,
    /// and `amount / count` is the average hit.
    pub spells: Vec<Row>,
    pub timeline: Timeline,
    /// v18: the drilled ability's own curve for THIS side, present iff the
    /// cursor names a spell this player actually cast. Drawn as the focus
    /// over `timeline` ghosted, exactly like the meter's ability drill.
    pub spell_timeline: Option<Timeline>,
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
        /// Daemon-side error/notice for the footer. When the segment is
        /// still being parsed this is [`loading_status`] and the rows are a
        /// placeholder — interactive clients paint it, request/response
        /// clients wait through it via [`is_loading_status`].
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
    /// R12: answers a `Cursor::Compare`. Coalesced by `seq` exactly like
    /// `Snapshot`, and equally idempotent — a lagging client is caught up by
    /// dropping the stale ones.
    CompareSnapshot {
        seq: u64,
        segment: SegmentRef,
        id: Option<SegmentId>,
        info: SegmentInfo,
        /// Boxed: a side carries a Row, a spell table and a timeline, and
        /// inlining two of them would make every `DaemonMsg` — including the
        /// 10 Hz meter snapshots — that big.
        a: Box<CompareSide>,
        b: Box<CompareSide>,
        /// v12: the window this snapshot's tables answer, echoed from the
        /// cursor — a renderer gates its zoomed view on the echo, never on
        /// what it last asked for, so a stale in-flight snapshot cannot pair
        /// full-fight tables with a zoomed graph.
        range: Option<(u32, u32)>,
        source: Option<String>,
        status: Option<String>,
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
        SegmentKind::Overall => 2,
    }
}

fn kind_from(b: u8) -> Result<SegmentKind> {
    Ok(match b {
        0 => SegmentKind::Encounter,
        1 => SegmentKind::Trash,
        2 => SegmentKind::Overall,
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
    // Blizzard specID, 0 = none. Sent as the raw id (not an enum code) so an
    // unknown value degrades to `None` on decode instead of erroring.
    wire::put_u16(buf, r.spec.map_or(0, |s| s.id() as u16));
    wire::put_u64(buf, r.count);
    wire::put_u64(buf, r.crits);
    wire::put_opt(buf, r.hp.as_ref(), |b, (cur, max)| {
        wire::put_u64(b, *cur);
        wire::put_u64(b, *max);
    });
    wire::put_bool(buf, r.gain);
    // v9: the spell id behind a by-spell label, 0 = none (icon lookup).
    wire::put_u32(buf, r.spell_id);
    // v10 (R13): the player fought on the hostile side (arena team split).
    wire::put_bool(buf, r.enemy);
    // v15: the spell's school bitmask (by-spell rows), 0 = none — the raw
    // log value, so unknown future schools pass through untouched.
    wire::put_u32(buf, r.school);
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
        spec: Spec::from_id(rd.u16()? as u32),
        count: rd.u64()?,
        crits: rd.u64()?,
        hp: rd.opt(|r| Ok((r.u64()?, r.u64()?)))?,
        gain: rd.bool()?,
        spell_id: rd.u32()?,
        enemy: rd.bool()?,
        school: rd.u32()?,
    })
}

fn put_info(buf: &mut Vec<u8>, i: &SegmentInfo) {
    wire::put_u8(buf, kind_code(i.kind));
    wire::put_str(buf, &i.name);
    wire::put_i64(buf, i.start_ms);
    wire::put_i64(buf, i.duration_ms);
    wire::put_opt(buf, i.success.as_ref(), |b, s| wire::put_bool(b, *s));
    wire::put_bool(buf, i.live);
    wire::put_opt(buf, i.instance.as_ref(), |b, v| wire::put_u32(b, *v));
    wire::put_opt(buf, i.pars_ms.as_ref(), put_pars);
    // v11 (R13): arena match — success reads WIN/LOSS.
    wire::put_bool(buf, i.arena);
}

fn get_info(rd: &mut Reader) -> Result<SegmentInfo> {
    Ok(SegmentInfo {
        kind: kind_from(rd.u8()?)?,
        name: rd.string()?,
        start_ms: rd.i64()?,
        duration_ms: rd.i64()?,
        success: rd.opt(|r| r.bool())?,
        live: rd.bool()?,
        instance: rd.opt(|r| r.u32())?,
        pars_ms: rd.opt(get_pars)?,
        arena: rd.bool()?,
    })
}

fn put_pars(buf: &mut Vec<u8>, p: &(i64, i64, i64)) {
    wire::put_i64(buf, p.0);
    wire::put_i64(buf, p.1);
    wire::put_i64(buf, p.2);
}

fn get_pars(rd: &mut Reader) -> Result<(i64, i64, i64)> {
    Ok((rd.i64()?, rd.i64()?, rd.i64()?))
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
    wire::put_opt(buf, r.instance.as_ref(), |b, v| wire::put_u32(b, *v));
    wire::put_opt(buf, r.pars_ms.as_ref(), put_pars);
    // v11 (R13): arena match — success reads WIN/LOSS.
    wire::put_bool(buf, r.arena);
}

fn get_list_row(rd: &mut Reader) -> Result<ListRow> {
    Ok(ListRow {
        kind: kind_from(rd.u8()?)?,
        name: rd.string()?,
        start_ms: rd.i64()?,
        success: rd.opt(|r| r.bool())?,
        duration_ms: rd.i64()?,
        live: rd.bool()?,
        instance: rd.opt(|r| r.u32())?,
        pars_ms: rd.opt(get_pars)?,
        arena: rd.bool()?,
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
            spell,
        } => {
            wire::put_u8(buf, 1);
            put_segment_ref(buf, *segment);
            wire::put_u8(buf, view_code(*view));
            wire::put_opt(buf, top_n.as_ref(), |b, n| wire::put_u32(b, *n));
            wire::put_opt(buf, drill.as_ref(), |b, d| wire::put_str(b, d));
            wire::put_opt(buf, spell.as_ref(), |b, s| wire::put_str(b, s));
        }
        Cursor::Compare {
            segment,
            a,
            b,
            range,
            spell,
        } => {
            wire::put_u8(buf, 2);
            put_segment_ref(buf, *segment);
            wire::put_str(buf, a);
            wire::put_str(buf, b);
            put_range(buf, *range);
            wire::put_opt(buf, spell.as_ref(), |b, s| wire::put_str(b, s));
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
            spell: rd.opt(|r| r.string())?,
        }),
        2 => Ok(Cursor::Compare {
            segment: get_segment_ref(rd)?,
            a: rd.string()?,
            b: rd.string()?,
            range: get_range(rd)?,
            spell: rd.opt(|r| r.string())?,
        }),
        b => Err(DecodeError::BadTag(b)),
    }
}

fn mark_kind_code(k: MarkKind) -> u8 {
    k.code()
}

fn mark_kind_from(b: u8) -> Result<MarkKind> {
    MarkKind::from_code(b).ok_or(DecodeError::BadTag(b))
}

fn put_mark(buf: &mut Vec<u8>, m: &Mark) {
    wire::put_i64(buf, m.at_ms);
    wire::put_u8(buf, mark_kind_code(m.kind));
    wire::put_str(buf, &m.label);
    wire::put_u32(buf, m.spell_id);
    wire::put_i64(buf, m.dur_ms);
}

fn get_mark(rd: &mut Reader) -> Result<Mark> {
    Ok(Mark {
        at_ms: rd.i64()?,
        kind: mark_kind_from(rd.u8()?)?,
        label: rd.string()?,
        spell_id: rd.u32()?,
        dur_ms: rd.i64()?,
    })
}

/// v12: a compare window, `lo..hi` ms from the segment start.
fn put_range(buf: &mut Vec<u8>, r: Option<(u32, u32)>) {
    wire::put_opt(buf, r.as_ref(), |b, (lo, hi)| {
        wire::put_u32(b, *lo);
        wire::put_u32(b, *hi);
    });
}

fn get_range(rd: &mut Reader) -> Result<Option<(u32, u32)>> {
    rd.opt(|r| Ok((r.u32()?, r.u32()?)))
}

fn put_timeline(buf: &mut Vec<u8>, t: &Timeline) {
    wire::put_u32(buf, t.bucket_ms);
    wire::put_vec(buf, &t.buckets, |b, v| wire::put_u64(b, *v));
    wire::put_vec(buf, &t.marks, put_mark);
}

fn get_timeline(rd: &mut Reader) -> Result<Timeline> {
    Ok(Timeline {
        bucket_ms: rd.u32()?,
        buckets: rd.vec(|r| r.u64())?,
        marks: rd.vec(get_mark)?,
    })
}

fn put_compare_side(buf: &mut Vec<u8>, s: &CompareSide) {
    wire::put_str(buf, &s.guid);
    put_row(buf, &s.total);
    wire::put_vec(buf, &s.spells, put_row);
    put_timeline(buf, &s.timeline);
    wire::put_opt(buf, s.spell_timeline.as_ref(), put_timeline);
}

fn get_compare_side(rd: &mut Reader) -> Result<CompareSide> {
    Ok(CompareSide {
        guid: rd.string()?,
        total: get_row(rd)?,
        spells: rd.vec(get_row)?,
        timeline: get_timeline(rd)?,
        spell_timeline: rd.opt(get_timeline)?,
    })
}

fn put_breakdown(buf: &mut Vec<u8>, b: &Breakdown) {
    wire::put_vec(buf, &b.by_spell, put_row);
    wire::put_vec(buf, &b.by_target, put_row);
    wire::put_opt(buf, b.timeline.as_ref(), put_timeline);
    wire::put_opt(buf, b.spell_timeline.as_ref(), put_timeline);
    wire::put_opt(buf, b.spell_targets.as_ref(), |b, v| {
        wire::put_vec(b, v, put_row)
    });
}

fn get_breakdown(rd: &mut Reader) -> Result<Breakdown> {
    Ok(Breakdown {
        by_spell: rd.vec(get_row)?,
        by_target: rd.vec(get_row)?,
        timeline: rd.opt(get_timeline)?,
        spell_timeline: rd.opt(get_timeline)?,
        spell_targets: rd.opt(|r| r.vec(get_row))?,
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
const T_DISCARD_TRASH: u8 = 0x06;

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
            ClientMsg::DiscardTrash => T_DISCARD_TRASH,
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
            T_DISCARD_TRASH => ClientMsg::DiscardTrash,
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
const T_COMPARE_SNAPSHOT: u8 = 0x89;

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
            DaemonMsg::CompareSnapshot {
                seq,
                segment,
                id,
                info,
                a,
                b,
                range,
                source,
                status,
            } => {
                wire::put_u64(&mut body, *seq);
                put_segment_ref(&mut body, *segment);
                wire::put_opt(&mut body, id.as_ref(), |b, i| wire::put_u64(b, i.0));
                put_info(&mut body, info);
                put_compare_side(&mut body, a);
                put_compare_side(&mut body, b);
                put_range(&mut body, *range);
                wire::put_opt(&mut body, source.as_ref(), |b, s| wire::put_str(b, s));
                wire::put_opt(&mut body, status.as_ref(), |b, s| wire::put_str(b, s));
                T_COMPARE_SNAPSHOT
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
            T_COMPARE_SNAPSHOT => DaemonMsg::CompareSnapshot {
                seq: rd.u64()?,
                segment: get_segment_ref(&mut rd)?,
                id: rd.opt(|r| Ok(SegmentId(r.u64()?)))?,
                info: get_info(&mut rd)?,
                a: Box::new(get_compare_side(&mut rd)?),
                b: Box::new(get_compare_side(&mut rd)?),
                range: get_range(&mut rd)?,
                source: rd.opt(|r| r.string())?,
                status: rd.opt(|r| r.string())?,
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

/// The status text a snapshot carries while its segment is still being
/// parsed — the hub's immediate placeholder answer to a watch on a cold
/// segment (the real snapshot follows when the loader delivers). One
/// producer, one predicate: the daemon renders it with [`loading_status`]
/// and request/response clients detect it with [`is_loading_status`], so
/// the two can never drift apart. The predicate keys on the prefix/suffix
/// pair rather than reconstructing the exact name, because the live-visit
/// placeholder names its status from the scanned prefix segment while its
/// info comes from the live meter — same visit, not always the same string.
pub fn loading_status(name: &str) -> String {
    format!("{LOADING_PREFIX}{name}{LOADING_SUFFIX}")
}

/// True for strings produced by [`loading_status`]. The only other status
/// texts on the wire are tail errors, `"{path}: {io error}"`, which cannot
/// start with the prefix.
pub fn is_loading_status(status: &str) -> bool {
    status.starts_with(LOADING_PREFIX) && status.ends_with(LOADING_SUFFIX)
}

const LOADING_PREFIX: &str = "loading ";
const LOADING_SUFFIX: &str = "…";

#[cfg(test)]
mod loading_status_tests {
    use super::*;

    #[test]
    fn the_predicate_matches_exactly_what_the_producer_makes() {
        assert!(is_loading_status(&loading_status("Mythic Gallywix")));
        assert!(is_loading_status(&loading_status("")));
        // Tail errors — the only other status texts — never match.
        assert!(!is_loading_status(
            "/logs/WoWCombatLog.txt: permission denied"
        ));
        assert!(!is_loading_status("loading without the ellipsis"));
    }
}
