//! The messages. Every variant encodes to one frame (`wire::frame`) and
//! decodes from `(tag, body)`. Changing the shape of anything here — fields,
//! order, enum codes — is a `PROTO_VERSION` bump; the golden-bytes tests
//! exist to make that impossible to do by accident.

use wowdps_model::{
    Class, Encounter, GearItem, ListRow, Loadout, Mark, MarkKind, MissKind, Mitigation, Role, Row,
    SegmentId, SegmentInfo, SegmentKind, Spec, TalentPick, Timeline, View,
};

use crate::history::{CardPlayer, FightCard, FightKind, KeyInfo, PlayerSupport};
use crate::wire::{self, DecodeError, Reader, Result};

/// Version of the whole wire surface. Embedded in the socket path, so a
/// mismatch is structurally impossible rather than diagnosed at handshake.
pub const PROTO_VERSION: u16 = 23;

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
    /// v19: one player's COMBATANT_INFO loadout for one segment, a one-shot
    /// like `GetStatus`. The daemon resolves `segment` exactly as a `Watch`
    /// would — a cold segment is loaded and the reply deferred, never
    /// answered with a placeholder.
    GetLoadout {
        req_id: u32,
        segment: SegmentRef,
        /// The meter row's `key`.
        guid: String,
    },
    /// v20: one of the history store's fixed questions (roadmap item 1). A
    /// one-shot with `GetLoadout` semantics: always answered, never an
    /// error — a disabled store answers empty and `Status` says why.
    GetHistory {
        req_id: u32,
        query: HistoryQuery,
    },
    /// v20: one stored fight — its card and the requested view's rows,
    /// plus the drilled player's breakdown when the details tier has it.
    GetFight {
        req_id: u32,
        fight_id: String,
        view: View,
        drill: Option<String>,
        /// v20: on a key, one member boss — its name (case-insensitive) or
        /// 0-based index into the card's `bosses` — parsed from the log on
        /// demand and answered with the boss's own rows / breakdown.
        boss: Option<String>,
    },
    /// v20: protect (or release) a stored fight from retention.
    PinFight {
        req_id: u32,
        fight_id: String,
        pinned: bool,
    },
    /// v20: queue an import sweep of one log or a directory of logs
    /// (`wowdps history import`). Answered with `HistoryAnswer::Imported`.
    ImportLog {
        req_id: u32,
        path: String,
    },
    /// v20: re-derive stored cards from their logs — one fight by id, or
    /// every pull of a boss + difficulty — rewriting each in place (pin and
    /// annotations kept) so a ruling change (R16) reaches old records.
    /// Answered by `History` / `Regraded { queued }`; the rewrites land
    /// through the import queue.
    Regrade {
        req_id: u32,
        fight_id: Option<String>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        /// Every card of this kind (with the other filters): `key` regrades
        /// all keystone Σs, which have no encounter id to select by.
        kind: Option<FightKind>,
    },
}

/// v20: how `HistoryQuery::Fights` orders its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FightSort {
    Newest,
    /// Shortest duration first; best kill = `Fastest` + `limit: 1`.
    Fastest,
    /// The owner's highest damage per second first.
    OwnerPerSec,
}

/// v20: how `HistoryQuery::Trend` groups fights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendBucket {
    /// One point per fight.
    None,
    /// UTC days.
    Day,
    /// UTC weeks (Monday-based).
    Week,
}

/// v22 (R17, step 2b): what a `HistoryQuery::Trend` point measures. Every
/// measure reads the card alone: `Dps` / `Hps` are the v20 Damage /
/// Healing views; `Dtps` is `CardPlayer::dtps` (amount = `taken`);
/// `MitigatedPct` is `CardPlayer::mitigated_pct()` (amount = `mitigated`);
/// v23 (R19, step 3b): `EffectiveDps` is `CardPlayer::effective_dps(
/// duration)` (amount = `effective()`) — equal to `Dps` on a fight without
/// support, so it is the DPS role's default measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendMeasure {
    Dps,
    Hps,
    Dtps,
    MitigatedPct,
    EffectiveDps,
}

impl TrendMeasure {
    /// The JSON / CLI spelling: `dps`, `hps`, `dtps`, `mitigated_pct`,
    /// `effective_dps`.
    pub fn name(self) -> &'static str {
        match self {
            TrendMeasure::Dps => "dps",
            TrendMeasure::Hps => "hps",
            TrendMeasure::Dtps => "dtps",
            TrendMeasure::MitigatedPct => "mitigated_pct",
            TrendMeasure::EffectiveDps => "effective_dps",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "dps" => TrendMeasure::Dps,
            "hps" => TrendMeasure::Hps,
            "dtps" => TrendMeasure::Dtps,
            "mitigated_pct" => TrendMeasure::MitigatedPct,
            "effective_dps" => TrendMeasure::EffectiveDps,
            _ => return None,
        })
    }
}

/// v20: the fixed questions the daemon answers from its card index.
#[derive(Debug, Clone, PartialEq)]
pub enum HistoryQuery {
    Fights {
        encounter: Option<u32>,
        difficulty: Option<u32>,
        /// Only fights this player was in.
        guid: Option<String>,
        since_utc_ms: Option<i64>,
        kind: Option<FightKind>,
        sort: FightSort,
        limit: u32,
        /// Paging: only fights that sort AFTER this id in the answer's
        /// order (the last id of the previous page). Unknown id = from the
        /// top.
        after_id: Option<String>,
        /// v22: only fights the SUBJECT (`guid`, else the owner) played
        /// this role in, by their spec on the card. With no subject (owner
        /// uninferred and no `guid`) the filter is a no-op.
        role: Option<Role>,
    },
    Progression {
        encounter: u32,
        difficulty: u32,
        /// v20: bucket nights by LOCAL day starting at this hour (a raid
        /// evening never straddles it), using each card's log timezone;
        /// `None` = UTC calendar days.
        local_cutover_hour: Option<u8>,
    },
    Trend {
        guid: String,
        /// Blizzard spec id, `None` = every spec.
        spec: Option<u32>,
        encounter: Option<u32>,
        difficulty: Option<u32>,
        /// v22: which card measure the points carry (replaced the v20–21
        /// `view: View` in the same position, one byte).
        measure: TrendMeasure,
        bucket: TrendBucket,
        since_utc_ms: Option<i64>,
        limit: u32,
        /// v20: `Day` / `Week` buckets on LOCAL days starting at this hour;
        /// `None` = UTC.
        local_cutover_hour: Option<u8>,
    },
}

/// v20: one night of a boss's progression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Night {
    /// Midnight UTC of the day.
    pub day_utc_ms: i64,
    pub pulls: u32,
    pub kill: bool,
    /// How many of the night's pulls were kills (farm nights kill more
    /// than once).
    pub kills: u32,
    /// R16: the lowest boss health any pull that night reached.
    pub best_pct: Option<u16>,
    /// The night's log timezone (its first card's), so a reader can name
    /// the local calendar date of a local-day bucket.
    pub tz_min: Option<i16>,
}

/// v20: one point of a trend — a fight, or a day/week of them averaged.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendPoint {
    pub bucket_utc_ms: i64,
    /// The fight (or, bucketed, the newest fight in the bucket).
    pub fight_id: String,
    pub spec: Option<u32>,
    /// The measure's numerator: damage / healing / taken (`Dtps`) /
    /// mitigated (`MitigatedPct`). A `Day` / `Week` bucket sums it.
    pub amount: u64,
    /// The measure's value: dps, hps, dtps — or, for `MitigatedPct`, the
    /// percentage itself. A `Day` / `Week` bucket folds it as a running
    /// mean of the per-fight values (a mean of pcts for `MitigatedPct`,
    /// exactly as Dps-by-day is already a mean of rates), never
    /// `amount / duration_ms`.
    pub per_sec: f64,
    pub duration_ms: i64,
    /// Fights folded into this point.
    pub n: u32,
    /// The bucket's log timezone (its newest fight's).
    pub tz_min: Option<i16>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HistoryAnswer {
    Fights {
        cards: Vec<FightCard>,
        /// Matches before `limit` and `after_id` were applied.
        total: u32,
    },
    Progression {
        pulls: u32,
        kills: u32,
        first_kill: Option<Box<FightCard>>,
        nights: Vec<Night>,
        median_kill_ms: Option<i64>,
    },
    Trend(Vec<TrendPoint>),
    Pinned {
        fight_id: String,
        pinned: bool,
    },
    Imported {
        queued: u32,
    },
    /// v20: how many cards a `Regrade` queued for rewriting.
    Regraded {
        queued: u32,
    },
}

/// v20: a stored fight as `GetFight` returns it — the same shape a live
/// snapshot has, so a reader needs no second path.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredFight {
    pub card: FightCard,
    pub rows: Vec<Row>,
    pub breakdown: Option<Breakdown>,
    /// The deepest tier on disk for this fight: 1 card only (rows evicted),
    /// 2 card + rows, 3 card + rows + details. What a drill can be served
    /// from.
    pub tier: u8,
    /// The drilled player has a death recap in the rows tier.
    pub has_recap: bool,
    /// The drilled player's logged loadout, from the loadouts tier.
    pub loadout: Option<Loadout>,
    /// v23 (R19, step 3b): the drilled player's support block from the
    /// rows tier — shares given / received and their target table;
    /// `None` when they neither gave nor received any, or without a drill.
    pub support: Option<PlayerSupport>,
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
    /// v21 (R17): the drilled player's mitigation record — partial / full
    /// absorbed and blocked amounts, overkill, the stagger pair and the
    /// per-kind miss counts. Present iff the drilled view is Taken.
    pub mitigation: Option<Mitigation>,
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

/// v20: the history store as `Status` reports it (roadmap item 1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryStatus {
    /// A store is configured and its thread is up.
    pub enabled: bool,
    /// Cards in the in-memory index.
    pub fights: u32,
    /// Writes dropped because the hub → history queue was full.
    pub dropped: u32,
    /// Import jobs queued or in flight.
    pub importing: u32,
    /// "Me" was inferred (no `history_characters`) and resolved.
    pub owner_inferred: bool,
    /// Why the store is disabled, or the latest write/read failure.
    pub error: Option<String>,
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

// `Fight` (a whole `StoredFight`: card + rows + breakdown + loadout + the
// v23 support block) outweighs `Snapshot` by more than clippy's 200 bytes.
// It is a one-shot answer built once per request, never a hot-path value,
// so — like `TailEvent::Index` — it is not boxed.
#[allow(clippy::large_enum_variant)]
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
        /// v20: the tailed log's identity (`proto::history::log_id`), once its
        /// header line is complete — with a row's `start_ms` it names the
        /// row's history-store fight id. `None` before the header lands and
        /// while no log is followed.
        log_id: Option<u64>,
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
        /// v20: the history store's state.
        history: HistoryStatus,
    },
    /// Overlay lifecycle command from the supervisor.
    SetVisible(bool),
    Fatal(String),
    /// v19: answers `GetLoadout`. `None` for an unknown guid, a player whose
    /// COMBATANT_INFO never fired, or a failed segment load — never an error.
    /// Raw log data only: resolving picks against the talent dataset stays
    /// client-side (R14).
    Loadout {
        req_id: u32,
        /// Echoed from the request, so a client with several in flight can
        /// match without a table.
        guid: String,
        loadout: Option<Loadout>,
    },
    /// v20: answers `GetHistory`, `PinFight` and `ImportLog`.
    History {
        req_id: u32,
        answer: HistoryAnswer,
    },
    /// v20: answers `GetFight`; `None` for an unknown or evicted fight.
    Fight {
        req_id: u32,
        fight: Option<StoredFight>,
    },
    /// v20: unsolicited, to every session, whenever the store writes a
    /// fight — like `SegmentList`, so a history screen knows to refresh.
    HistoryChanged {
        fight_id: String,
    },
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
        // v21 (R17): damage taken.
        6 => View::Taken,
        _ => return Err(DecodeError::BadTag(b)),
    })
}

/// v22: `TrendMeasure` codes 0..3 in declaration order; v23 adds
/// `EffectiveDps` = 4.
fn measure_code(m: TrendMeasure) -> u8 {
    match m {
        TrendMeasure::Dps => 0,
        TrendMeasure::Hps => 1,
        TrendMeasure::Dtps => 2,
        TrendMeasure::MitigatedPct => 3,
        TrendMeasure::EffectiveDps => 4,
    }
}

fn measure_from(b: u8) -> Result<TrendMeasure> {
    Ok(match b {
        0 => TrendMeasure::Dps,
        1 => TrendMeasure::Hps,
        2 => TrendMeasure::Dtps,
        3 => TrendMeasure::MitigatedPct,
        4 => TrendMeasure::EffectiveDps,
        _ => return Err(DecodeError::BadTag(b)),
    })
}

/// v22: `Role` codes — Tank 0, Healer 1, Dps 2.
fn role_code(r: Role) -> u8 {
    match r {
        Role::Tank => 0,
        Role::Healer => 1,
        Role::Dps => 2,
    }
}

fn role_from(b: u8) -> Result<Role> {
    Ok(match b {
        0 => Role::Tank,
        1 => Role::Healer,
        2 => Role::Dps,
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
    // v20: encounter identity (id, difficulty, group size).
    wire::put_opt(buf, i.encounter.as_ref(), put_encounter);
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
        encounter: rd.opt(get_encounter)?,
    })
}

fn put_encounter(buf: &mut Vec<u8>, e: &Encounter) {
    wire::put_u32(buf, e.id);
    wire::put_u32(buf, e.difficulty);
    wire::put_u32(buf, e.group_size);
}

fn get_encounter(rd: &mut Reader) -> Result<Encounter> {
    Ok(Encounter {
        id: rd.u32()?,
        difficulty: rd.u32()?,
        group_size: rd.u32()?,
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
    // v20: encounter identity (id, difficulty, group size).
    wire::put_opt(buf, r.encounter.as_ref(), put_encounter);
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
        encounter: rd.opt(get_encounter)?,
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
        // v24 (wire slice) reads the caster; until then the field is empty.
        src: String::new(),
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
    // v21: a Breakdown is embedded inside Snapshot / StoredFight with fields
    // after it, so this is NOT a frame-trailing option — the presence byte
    // is always written, `None` included.
    wire::put_opt(buf, b.mitigation.as_ref(), put_mitigation);
}

fn get_breakdown(rd: &mut Reader) -> Result<Breakdown> {
    Ok(Breakdown {
        by_spell: rd.vec(get_row)?,
        by_target: rd.vec(get_row)?,
        timeline: rd.opt(get_timeline)?,
        spell_timeline: rd.opt(get_timeline)?,
        spell_targets: rd.opt(|r| r.vec(get_row))?,
        mitigation: rd.opt(get_mitigation)?,
    })
}

/// v21 (R17): the six u64 amounts in declaration order (`absorbed`,
/// `blocked`, `absorbed_full`, `blocked_full`, `stagger`, `stagger_ticked`),
/// then the ten miss counts as u32 in `MissKind::ALL` order (=
/// `MissKind::index` order). Fixed 88 bytes, no counts — nothing an
/// attacker can size. (Overkill is the R9 recap's, per death — not here.)
fn put_mitigation(buf: &mut Vec<u8>, m: &Mitigation) {
    wire::put_u64(buf, m.absorbed);
    wire::put_u64(buf, m.blocked);
    wire::put_u64(buf, m.absorbed_full);
    wire::put_u64(buf, m.blocked_full);
    wire::put_u64(buf, m.stagger);
    wire::put_u64(buf, m.stagger_ticked);
    for kind in MissKind::ALL {
        wire::put_u32(buf, m.misses.get(kind.index()).copied().unwrap_or(0));
    }
}

fn get_mitigation(rd: &mut Reader) -> Result<Mitigation> {
    let mut m = Mitigation {
        absorbed: rd.u64()?,
        blocked: rd.u64()?,
        absorbed_full: rd.u64()?,
        blocked_full: rd.u64()?,
        stagger: rd.u64()?,
        stagger_ticked: rd.u64()?,
        misses: [0; MissKind::COUNT],
    };
    for kind in MissKind::ALL {
        let n = rd.u32()?;
        if let Some(slot) = m.misses.get_mut(kind.index()) {
            *slot = n;
        }
    }
    Ok(m)
}

fn put_talent_pick(buf: &mut Vec<u8>, t: &TalentPick) {
    wire::put_u32(buf, t.node_id);
    wire::put_u32(buf, t.entry_id);
    wire::put_u32(buf, t.rank);
}

fn get_talent_pick(rd: &mut Reader) -> Result<TalentPick> {
    Ok(TalentPick {
        node_id: rd.u32()?,
        entry_id: rd.u32()?,
        rank: rd.u32()?,
    })
}

fn put_gear_item(buf: &mut Vec<u8>, g: &GearItem) {
    wire::put_u32(buf, g.item_id);
    wire::put_u32(buf, g.ilvl);
    wire::put_vec(buf, &g.enchants, |b, v| wire::put_u32(b, *v));
    wire::put_vec(buf, &g.bonus_ids, |b, v| wire::put_u32(b, *v));
    wire::put_vec(buf, &g.gems, |b, v| wire::put_u32(b, *v));
}

fn get_gear_item(rd: &mut Reader) -> Result<GearItem> {
    Ok(GearItem {
        item_id: rd.u32()?,
        ilvl: rd.u32()?,
        enchants: rd.vec(|r| r.u32())?,
        bonus_ids: rd.vec(|r| r.u32())?,
        gems: rd.vec(|r| r.u32())?,
    })
}

/// The v19 wire encoding of a loadout on its own — what the history store
/// content-addresses (`proto::history::loadout_hash`), so the same build
/// hashes the same whether it came off the socket or out of a file.
pub fn loadout_bytes(l: &Loadout) -> Vec<u8> {
    let mut buf = Vec::new();
    put_loadout(&mut buf, l);
    buf
}

fn put_loadout(buf: &mut Vec<u8>, l: &Loadout) {
    // Blizzard specID as the raw id, 0 = none, like `Row.spec`.
    wire::put_u16(buf, l.spec_id.map_or(0, |s| s as u16));
    wire::put_vec(buf, &l.talents, put_talent_pick);
    wire::put_vec(buf, &l.gear, put_gear_item);
}

fn get_loadout(rd: &mut Reader) -> Result<Loadout> {
    let spec = rd.u16()?;
    Ok(Loadout {
        spec_id: (spec != 0).then_some(spec as u32),
        talents: rd.vec(get_talent_pick)?,
        gear: rd.vec(get_gear_item)?,
    })
}

fn put_history_status(buf: &mut Vec<u8>, h: &HistoryStatus) {
    wire::put_bool(buf, h.enabled);
    wire::put_u32(buf, h.fights);
    wire::put_u32(buf, h.dropped);
    wire::put_u32(buf, h.importing);
    wire::put_bool(buf, h.owner_inferred);
    wire::put_opt(buf, h.error.as_ref(), |b, s| wire::put_str(b, s));
}

fn get_history_status(rd: &mut Reader) -> Result<HistoryStatus> {
    Ok(HistoryStatus {
        enabled: rd.bool()?,
        fights: rd.u32()?,
        dropped: rd.u32()?,
        importing: rd.u32()?,
        owner_inferred: rd.bool()?,
        error: rd.opt(|r| r.string())?,
    })
}

// ---- v20: history payloads ----------------------------------------------------

fn put_opt_u32(buf: &mut Vec<u8>, v: Option<u32>) {
    wire::put_opt(buf, v.as_ref(), |b, n| wire::put_u32(b, *n));
}

fn put_opt_i64(buf: &mut Vec<u8>, v: Option<i64>) {
    wire::put_opt(buf, v.as_ref(), |b, n| wire::put_i64(b, *n));
}

fn put_opt_str(buf: &mut Vec<u8>, v: Option<&str>) {
    // The same presence-byte shape `put_opt` writes, over an unsized str.
    wire::put_bool(buf, v.is_some());
    if let Some(s) = v {
        wire::put_str(buf, s);
    }
}

fn fight_kind_code(k: FightKind) -> u8 {
    match k {
        FightKind::Encounter => 0,
        FightKind::Arena => 1,
        FightKind::Key => 2,
        FightKind::Overall => 3,
        FightKind::Trash => 4,
    }
}

fn fight_kind_from(b: u8) -> Result<FightKind> {
    Ok(match b {
        0 => FightKind::Encounter,
        1 => FightKind::Arena,
        2 => FightKind::Key,
        3 => FightKind::Overall,
        4 => FightKind::Trash,
        other => return Err(DecodeError::BadTag(other)),
    })
}

fn put_card_player(buf: &mut Vec<u8>, p: &CardPlayer) {
    wire::put_str(buf, &p.guid);
    wire::put_str(buf, &p.name);
    wire::put_opt(buf, p.class.as_ref(), |b, c| {
        wire::put_u8(b, class_code(*c))
    });
    wire::put_u16(buf, p.spec.map_or(0, |s| s.id() as u16));
    wire::put_opt(buf, p.loadout.as_ref(), |b, h| wire::put_u64(b, *h));
    wire::put_bool(buf, p.logged);
    wire::put_bool(buf, p.enemy);
    wire::put_u64(buf, p.damage);
    wire::put_f64(buf, p.dps);
    wire::put_u64(buf, p.healing);
    wire::put_f64(buf, p.hps);
    wire::put_u32(buf, p.deaths);
    // v22 (R17, step 2b): the tank measures, trailing. `mitigated_pct` is
    // derived (`CardPlayer::mitigated_pct`) and never travels.
    wire::put_u64(buf, p.taken);
    wire::put_u64(buf, p.mitigated);
    wire::put_u64(buf, p.prevented);
    wire::put_f64(buf, p.dtps);
    // v23 (R19, step 3b): the healing split and the support scalars,
    // trailing, six u64 in declaration order (48 bytes). `effective_dps`
    // is derived (`CardPlayer::effective_dps`) and never travels.
    wire::put_u64(buf, p.overheal);
    wire::put_u64(buf, p.absorbed);
    wire::put_u64(buf, p.support_given);
    wire::put_u64(buf, p.support_received);
    wire::put_u64(buf, p.healed_received);
    wire::put_u64(buf, p.self_healed);
}

fn get_card_player(rd: &mut Reader) -> Result<CardPlayer> {
    Ok(CardPlayer {
        guid: rd.string()?,
        name: rd.string()?,
        class: rd.opt(|r| class_from(r.u8()?))?,
        spec: Spec::from_id(rd.u16()? as u32),
        loadout: rd.opt(|r| r.u64())?,
        logged: rd.bool()?,
        enemy: rd.bool()?,
        damage: rd.u64()?,
        dps: rd.f64()?,
        healing: rd.u64()?,
        hps: rd.f64()?,
        deaths: rd.u32()?,
        taken: rd.u64()?,
        mitigated: rd.u64()?,
        prevented: rd.u64()?,
        dtps: rd.f64()?,
        overheal: rd.u64()?,
        absorbed: rd.u64()?,
        support_given: rd.u64()?,
        support_received: rd.u64()?,
        healed_received: rd.u64()?,
        self_healed: rd.u64()?,
    })
}

fn put_card(buf: &mut Vec<u8>, c: &FightCard) {
    wire::put_u16(buf, c.schema);
    wire::put_str(buf, &c.id);
    wire::put_u64(buf, c.log);
    wire::put_u64(buf, c.content);
    wire::put_u8(buf, fight_kind_code(c.kind));
    wire::put_str(buf, &c.name);
    wire::put_opt(buf, c.encounter.as_ref(), put_encounter);
    wire::put_opt(buf, c.key.as_ref(), |b, k| {
        wire::put_u32(b, k.map_id);
        wire::put_u32(b, k.difficulty);
        put_opt_u32(b, k.level);
        wire::put_opt(b, k.completed.as_ref(), |b, v| wire::put_bool(b, *v));
    });
    wire::put_i64(buf, c.start_local_ms);
    wire::put_opt(buf, c.tz_min.as_ref(), |b, m| wire::put_u16(b, *m as u16));
    wire::put_i64(buf, c.start_utc_ms);
    wire::put_i64(buf, c.duration_ms);
    put_opt_i64(buf, c.official_ms);
    wire::put_opt(buf, c.pars_ms.as_ref(), put_pars);
    wire::put_opt(buf, c.success.as_ref(), |b, v| wire::put_bool(b, *v));
    wire::put_bool(buf, c.aborted);
    wire::put_u16(buf, c.build.0);
    wire::put_u16(buf, c.build.1);
    wire::put_u16(buf, c.build.2);
    wire::put_u8(buf, c.project_id);
    wire::put_u32(buf, c.log_version);
    put_opt_str(buf, c.owner.as_deref());
    wire::put_opt(buf, c.byte_range.as_ref(), |b, (s, e)| {
        wire::put_u64(b, *s);
        wire::put_u64(b, *e);
    });
    wire::put_bool(buf, c.pinned);
    wire::put_opt(buf, c.best_pct.as_ref(), |b, p| wire::put_u16(b, *p));
    wire::put_vec(buf, &c.players, put_card_player);
    wire::put_vec(buf, &c.bosses, |b, k| {
        wire::put_str(b, &k.name);
        wire::put_opt(b, k.encounter.as_ref(), put_encounter);
        wire::put_i64(b, k.start_utc_ms);
        wire::put_i64(b, k.duration_ms);
        wire::put_opt(b, k.success.as_ref(), |b, s| wire::put_bool(b, *s));
    });
}

fn get_card(rd: &mut Reader) -> Result<FightCard> {
    Ok(FightCard {
        schema: rd.u16()?,
        id: rd.string()?,
        log: rd.u64()?,
        content: rd.u64()?,
        kind: fight_kind_from(rd.u8()?)?,
        name: rd.string()?,
        encounter: rd.opt(get_encounter)?,
        key: rd.opt(|r| {
            Ok(KeyInfo {
                map_id: r.u32()?,
                difficulty: r.u32()?,
                level: r.opt(|r| r.u32())?,
                completed: r.opt(|r| r.bool())?,
            })
        })?,
        start_local_ms: rd.i64()?,
        tz_min: rd.opt(|r| Ok(r.u16()? as i16))?,
        start_utc_ms: rd.i64()?,
        duration_ms: rd.i64()?,
        official_ms: rd.opt(|r| r.i64())?,
        pars_ms: rd.opt(get_pars)?,
        success: rd.opt(|r| r.bool())?,
        aborted: rd.bool()?,
        build: (rd.u16()?, rd.u16()?, rd.u16()?),
        project_id: rd.u8()?,
        log_version: rd.u32()?,
        owner: rd.opt(|r| r.string())?,
        byte_range: rd.opt(|r| Ok((r.u64()?, r.u64()?)))?,
        pinned: rd.bool()?,
        best_pct: rd.opt(|r| r.u16())?,
        players: rd.vec(get_card_player)?,
        bosses: rd.vec(|r| {
            Ok(crate::history::KeyBoss {
                name: r.string()?,
                encounter: r.opt(get_encounter)?,
                start_utc_ms: r.i64()?,
                duration_ms: r.i64()?,
                success: r.opt(|r| r.bool())?,
            })
        })?,
    })
}

fn put_query(buf: &mut Vec<u8>, q: &HistoryQuery) {
    match q {
        HistoryQuery::Fights {
            encounter,
            difficulty,
            guid,
            since_utc_ms,
            kind,
            sort,
            limit,
            after_id,
            role,
        } => {
            wire::put_u8(buf, 0);
            put_opt_u32(buf, *encounter);
            put_opt_u32(buf, *difficulty);
            put_opt_str(buf, guid.as_deref());
            put_opt_i64(buf, *since_utc_ms);
            wire::put_opt(buf, kind.as_ref(), |b, k| {
                wire::put_u8(b, fight_kind_code(*k))
            });
            wire::put_u8(
                buf,
                match sort {
                    FightSort::Newest => 0,
                    FightSort::Fastest => 1,
                    FightSort::OwnerPerSec => 2,
                },
            );
            wire::put_u32(buf, *limit);
            put_opt_str(buf, after_id.as_deref());
            // v22: trailing Option<Role>.
            wire::put_opt(buf, role.as_ref(), |b, r| wire::put_u8(b, role_code(*r)));
        }
        HistoryQuery::Progression {
            encounter,
            difficulty,
            local_cutover_hour,
        } => {
            wire::put_u8(buf, 1);
            wire::put_u32(buf, *encounter);
            wire::put_u32(buf, *difficulty);
            wire::put_opt(buf, local_cutover_hour.as_ref(), |b, h| wire::put_u8(b, *h));
        }
        HistoryQuery::Trend {
            guid,
            spec,
            encounter,
            difficulty,
            measure,
            bucket,
            since_utc_ms,
            limit,
            local_cutover_hour,
        } => {
            wire::put_u8(buf, 2);
            wire::put_str(buf, guid);
            put_opt_u32(buf, *spec);
            put_opt_u32(buf, *encounter);
            put_opt_u32(buf, *difficulty);
            // v22: the measure byte sits where the v20 view byte did.
            wire::put_u8(buf, measure_code(*measure));
            wire::put_u8(
                buf,
                match bucket {
                    TrendBucket::None => 0,
                    TrendBucket::Day => 1,
                    TrendBucket::Week => 2,
                },
            );
            put_opt_i64(buf, *since_utc_ms);
            wire::put_u32(buf, *limit);
            wire::put_opt(buf, local_cutover_hour.as_ref(), |b, h| wire::put_u8(b, *h));
        }
    }
}

fn get_query(rd: &mut Reader) -> Result<HistoryQuery> {
    Ok(match rd.u8()? {
        0 => HistoryQuery::Fights {
            encounter: rd.opt(|r| r.u32())?,
            difficulty: rd.opt(|r| r.u32())?,
            guid: rd.opt(|r| r.string())?,
            since_utc_ms: rd.opt(|r| r.i64())?,
            kind: rd.opt(|r| fight_kind_from(r.u8()?))?,
            sort: match rd.u8()? {
                0 => FightSort::Newest,
                1 => FightSort::Fastest,
                2 => FightSort::OwnerPerSec,
                other => return Err(DecodeError::BadTag(other)),
            },
            limit: rd.u32()?,
            after_id: rd.opt(|r| r.string())?,
            role: rd.opt(|r| role_from(r.u8()?))?,
        },
        1 => HistoryQuery::Progression {
            encounter: rd.u32()?,
            difficulty: rd.u32()?,
            local_cutover_hour: rd.opt(|r| r.u8())?,
        },
        2 => HistoryQuery::Trend {
            guid: rd.string()?,
            spec: rd.opt(|r| r.u32())?,
            encounter: rd.opt(|r| r.u32())?,
            difficulty: rd.opt(|r| r.u32())?,
            measure: measure_from(rd.u8()?)?,
            bucket: match rd.u8()? {
                0 => TrendBucket::None,
                1 => TrendBucket::Day,
                2 => TrendBucket::Week,
                other => return Err(DecodeError::BadTag(other)),
            },
            since_utc_ms: rd.opt(|r| r.i64())?,
            limit: rd.u32()?,
            local_cutover_hour: rd.opt(|r| r.u8())?,
        },
        other => return Err(DecodeError::BadTag(other)),
    })
}

fn put_answer(buf: &mut Vec<u8>, a: &HistoryAnswer) {
    match a {
        HistoryAnswer::Fights { cards, total } => {
            wire::put_u8(buf, 0);
            wire::put_vec(buf, cards, put_card);
            wire::put_u32(buf, *total);
        }
        HistoryAnswer::Progression {
            pulls,
            kills,
            first_kill,
            nights,
            median_kill_ms,
        } => {
            wire::put_u8(buf, 1);
            wire::put_u32(buf, *pulls);
            wire::put_u32(buf, *kills);
            wire::put_opt(buf, first_kill.as_deref(), put_card);
            wire::put_vec(buf, nights, |b, n| {
                wire::put_i64(b, n.day_utc_ms);
                wire::put_u32(b, n.pulls);
                wire::put_bool(b, n.kill);
                wire::put_u32(b, n.kills);
                wire::put_opt(b, n.best_pct.as_ref(), |b, p| wire::put_u16(b, *p));
                wire::put_opt(b, n.tz_min.as_ref(), |b, t| wire::put_u16(b, *t as u16));
            });
            put_opt_i64(buf, *median_kill_ms);
        }
        HistoryAnswer::Trend(points) => {
            wire::put_u8(buf, 2);
            wire::put_vec(buf, points, |b, p| {
                wire::put_i64(b, p.bucket_utc_ms);
                wire::put_str(b, &p.fight_id);
                put_opt_u32(b, p.spec);
                wire::put_u64(b, p.amount);
                wire::put_f64(b, p.per_sec);
                wire::put_i64(b, p.duration_ms);
                wire::put_u32(b, p.n);
                wire::put_opt(b, p.tz_min.as_ref(), |b, t| wire::put_u16(b, *t as u16));
            });
        }
        HistoryAnswer::Pinned { fight_id, pinned } => {
            wire::put_u8(buf, 3);
            wire::put_str(buf, fight_id);
            wire::put_bool(buf, *pinned);
        }
        HistoryAnswer::Imported { queued } => {
            wire::put_u8(buf, 4);
            wire::put_u32(buf, *queued);
        }
        HistoryAnswer::Regraded { queued } => {
            wire::put_u8(buf, 5);
            wire::put_u32(buf, *queued);
        }
    }
}

fn get_answer(rd: &mut Reader) -> Result<HistoryAnswer> {
    Ok(match rd.u8()? {
        0 => HistoryAnswer::Fights {
            cards: rd.vec(get_card)?,
            total: rd.u32()?,
        },
        1 => HistoryAnswer::Progression {
            pulls: rd.u32()?,
            kills: rd.u32()?,
            first_kill: rd.opt(|r| get_card(r).map(Box::new))?,
            nights: rd.vec(|r| {
                Ok(Night {
                    day_utc_ms: r.i64()?,
                    pulls: r.u32()?,
                    kill: r.bool()?,
                    kills: r.u32()?,
                    best_pct: r.opt(|r| r.u16())?,
                    tz_min: r.opt(|r| Ok(r.u16()? as i16))?,
                })
            })?,
            median_kill_ms: rd.opt(|r| r.i64())?,
        },
        2 => HistoryAnswer::Trend(rd.vec(|r| {
            Ok(TrendPoint {
                bucket_utc_ms: r.i64()?,
                fight_id: r.string()?,
                spec: r.opt(|r| r.u32())?,
                amount: r.u64()?,
                per_sec: r.f64()?,
                duration_ms: r.i64()?,
                n: r.u32()?,
                tz_min: r.opt(|r| Ok(r.u16()? as i16))?,
            })
        })?),
        3 => HistoryAnswer::Pinned {
            fight_id: rd.string()?,
            pinned: rd.bool()?,
        },
        4 => HistoryAnswer::Imported { queued: rd.u32()? },
        5 => HistoryAnswer::Regraded { queued: rd.u32()? },
        other => return Err(DecodeError::BadTag(other)),
    })
}

fn put_stored_fight(buf: &mut Vec<u8>, f: &StoredFight) {
    put_card(buf, &f.card);
    wire::put_vec(buf, &f.rows, put_row);
    wire::put_opt(buf, f.breakdown.as_ref(), put_breakdown);
    wire::put_u8(buf, f.tier);
    wire::put_bool(buf, f.has_recap);
    wire::put_opt(buf, f.loadout.as_ref(), put_loadout);
    // v23: the drilled player's support block, trailing.
    wire::put_opt(buf, f.support.as_ref(), put_player_support);
}

fn get_stored_fight(rd: &mut Reader) -> Result<StoredFight> {
    Ok(StoredFight {
        card: get_card(rd)?,
        rows: rd.vec(get_row)?,
        breakdown: rd.opt(get_breakdown)?,
        tier: rd.u8()?,
        has_recap: rd.bool()?,
        loadout: rd.opt(get_loadout)?,
        support: rd.opt(get_player_support)?,
    })
}

/// v23: guid, the four share scalars as u64 in declaration order (given
/// damage, given healing, received damage, received healing), then the
/// target rows.
fn put_player_support(buf: &mut Vec<u8>, s: &PlayerSupport) {
    wire::put_str(buf, &s.guid);
    wire::put_u64(buf, s.given_damage);
    wire::put_u64(buf, s.given_healing);
    wire::put_u64(buf, s.received_damage);
    wire::put_u64(buf, s.received_healing);
    wire::put_vec(buf, &s.targets, put_row);
}

fn get_player_support(rd: &mut Reader) -> Result<PlayerSupport> {
    Ok(PlayerSupport {
        guid: rd.string()?,
        given_damage: rd.u64()?,
        given_healing: rd.u64()?,
        received_damage: rd.u64()?,
        received_healing: rd.u64()?,
        targets: rd.vec(get_row)?,
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
const T_GET_LOADOUT: u8 = 0x07;
const T_GET_HISTORY: u8 = 0x08;
const T_GET_FIGHT: u8 = 0x09;
const T_PIN_FIGHT: u8 = 0x0A;
const T_IMPORT_LOG: u8 = 0x0B;
const T_REGRADE: u8 = 0x0C;

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
            ClientMsg::GetLoadout {
                req_id,
                segment,
                guid,
            } => {
                wire::put_u32(&mut body, *req_id);
                put_segment_ref(&mut body, *segment);
                wire::put_str(&mut body, guid);
                T_GET_LOADOUT
            }
            ClientMsg::GetHistory { req_id, query } => {
                wire::put_u32(&mut body, *req_id);
                put_query(&mut body, query);
                T_GET_HISTORY
            }
            ClientMsg::GetFight {
                req_id,
                fight_id,
                view,
                drill,
                boss,
            } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_str(&mut body, fight_id);
                wire::put_u8(&mut body, view_code(*view));
                wire::put_opt(&mut body, drill.as_ref(), |b, d| wire::put_str(b, d));
                put_opt_str(&mut body, boss.as_deref());
                T_GET_FIGHT
            }
            ClientMsg::PinFight {
                req_id,
                fight_id,
                pinned,
            } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_str(&mut body, fight_id);
                wire::put_bool(&mut body, *pinned);
                T_PIN_FIGHT
            }
            ClientMsg::ImportLog { req_id, path } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_str(&mut body, path);
                T_IMPORT_LOG
            }
            ClientMsg::Regrade {
                req_id,
                fight_id,
                encounter,
                difficulty,
                kind,
            } => {
                wire::put_u32(&mut body, *req_id);
                put_opt_str(&mut body, fight_id.as_deref());
                put_opt_u32(&mut body, *encounter);
                put_opt_u32(&mut body, *difficulty);
                wire::put_opt(&mut body, kind.as_ref(), |b, k| {
                    wire::put_u8(b, fight_kind_code(*k))
                });
                T_REGRADE
            }
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
            T_GET_LOADOUT => ClientMsg::GetLoadout {
                req_id: rd.u32()?,
                segment: get_segment_ref(&mut rd)?,
                guid: rd.string()?,
            },
            T_GET_HISTORY => ClientMsg::GetHistory {
                req_id: rd.u32()?,
                query: get_query(&mut rd)?,
            },
            T_GET_FIGHT => ClientMsg::GetFight {
                req_id: rd.u32()?,
                fight_id: rd.string()?,
                view: view_from(rd.u8()?)?,
                drill: rd.opt(|r| r.string())?,
                boss: rd.opt(|r| r.string())?,
            },
            T_PIN_FIGHT => ClientMsg::PinFight {
                req_id: rd.u32()?,
                fight_id: rd.string()?,
                pinned: rd.bool()?,
            },
            T_IMPORT_LOG => ClientMsg::ImportLog {
                req_id: rd.u32()?,
                path: rd.string()?,
            },
            T_REGRADE => ClientMsg::Regrade {
                req_id: rd.u32()?,
                fight_id: rd.opt(|r| r.string())?,
                encounter: rd.opt(|r| r.u32())?,
                difficulty: rd.opt(|r| r.u32())?,
                kind: rd.opt(|r| fight_kind_from(r.u8()?))?,
            },
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
const T_LOADOUT: u8 = 0x8A;
const T_HISTORY: u8 = 0x8B;
const T_FIGHT: u8 = 0x8C;
const T_HISTORY_CHANGED: u8 = 0x8D;

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
                log_id,
            } => {
                wire::put_u64(&mut body, *seq);
                wire::put_vec(&mut body, entries, put_list_entry);
                wire::put_opt(&mut body, source.as_ref(), |b, s| wire::put_str(b, s));
                wire::put_bool(&mut body, *active);
                wire::put_opt(&mut body, log_id.as_ref(), |b, v| wire::put_u64(b, *v));
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
                history,
            } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_bool(&mut body, *game_running);
                wire::put_opt(&mut body, source.as_ref(), |b, s| wire::put_str(b, s));
                wire::put_u32(&mut body, *clients);
                wire::put_bool(&mut body, *linger);
                put_overlay_state(&mut body, overlay);
                // v20: trailing history status.
                put_history_status(&mut body, history);
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
            DaemonMsg::Loadout {
                req_id,
                guid,
                loadout,
            } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_str(&mut body, guid);
                wire::put_opt(&mut body, loadout.as_ref(), put_loadout);
                T_LOADOUT
            }
            DaemonMsg::History { req_id, answer } => {
                wire::put_u32(&mut body, *req_id);
                put_answer(&mut body, answer);
                T_HISTORY
            }
            DaemonMsg::Fight { req_id, fight } => {
                wire::put_u32(&mut body, *req_id);
                wire::put_opt(&mut body, fight.as_ref(), put_stored_fight);
                T_FIGHT
            }
            DaemonMsg::HistoryChanged { fight_id } => {
                wire::put_str(&mut body, fight_id);
                T_HISTORY_CHANGED
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
                log_id: rd.opt(|r| r.u64())?,
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
                history: get_history_status(&mut rd)?,
            },
            T_SET_VISIBLE => DaemonMsg::SetVisible(rd.bool()?),
            T_FATAL => DaemonMsg::Fatal(rd.string()?),
            T_LOADOUT => DaemonMsg::Loadout {
                req_id: rd.u32()?,
                guid: rd.string()?,
                loadout: rd.opt(get_loadout)?,
            },
            T_HISTORY => DaemonMsg::History {
                req_id: rd.u32()?,
                answer: get_answer(&mut rd)?,
            },
            T_FIGHT => DaemonMsg::Fight {
                req_id: rd.u32()?,
                fight: rd.opt(get_stored_fight)?,
            },
            T_HISTORY_CHANGED => DaemonMsg::HistoryChanged {
                fight_id: rd.string()?,
            },
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
