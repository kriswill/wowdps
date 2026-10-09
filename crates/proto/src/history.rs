//! The history store's record codec (roadmap item 1, `docs/spec-history-store.md`).
//!
//! The daemon writes one JSON document per file under
//! `$XDG_DATA_HOME/wowdps/history/v1/`; every reader — the daemon's own
//! in-memory index, `wowdps-history`'s DuckDB views, the mcp tools — parses
//! the same documents through this module. House rules:
//!
//! - **Summaries, never events.** Nothing here is keyed per event; every
//!   record is something `Meter` re-derives from the log.
//! - **Decode never panics.** `from_json` returns `None` only when the
//!   document has no identity (`schema` + `id`); every other missing field
//!   takes its default, so a `v1` document written before a field existed
//!   still reads after the field is added. Within `v1/` fields are only ever
//!   added.
//! - **Hashes are hex strings.** `Json::Num` is an `f64`; a 64-bit hash
//!   would lose bits, so every fnv64 travels as sixteen hex digits.
//! - Object key order is fixed by the encoders, so documents are
//!   byte-deterministic and golden-testable (`proto/tests/history.rs`).

use crate::json::Json;
use crate::lua::Lua;
use crate::obj;
use wowdps_model::LoadoutAura;
use wowdps_model::{
    Class, Encounter, EnergizeRow, GearItem, Loadout, Mark, MarkKind, MissKind, Mitigation, Role,
    Row, ShieldRow, Spec, StackBase, StackCell, StackingDebuff, TalentPick, Timeline, UptimeCell,
    View,
};
use wowdps_model::{Empower, GroupKind, PowerSeries, SpellGroup, SpellMeta, SpellPart, SpellTree};

/// Version of every document's shape. Independent of `PROTO_VERSION`: the
/// socket can move without the files moving. A record whose `schema` is
/// older than this is rewritten by the daemon on its next visit; a breaking
/// change is a new directory (`v2/`) plus a migrator, never in-place edits.
pub const HISTORY_SCHEMA: u16 = 1;

/// The seven views (R17's Taken last) in the order their rows are stored,
/// each with the key its rows sit under in a rows document.
/// Deliberately NOT every `View`: R24's `EnemyTaken` is a live meter and is
/// never stored — a card's rows are about the group.
pub const VIEW_KEYS: [(View, &str); 7] = [
    (View::Damage, "damage"),
    (View::Healing, "healing"),
    (View::Interrupts, "interrupts"),
    (View::CrowdControl, "cc"),
    (View::Dispels, "dispels"),
    (View::Deaths, "deaths"),
    (View::Taken, "taken"),
];

/// A stored view's key in [`VIEW_KEYS`] ("" for one never stored).
fn view_key(view: View) -> &'static str {
    VIEW_KEYS
        .iter()
        .find(|(v, _)| *v == view)
        .map_or("", |(_, k)| k)
}

/// The stored view a [`VIEW_KEYS`] key names.
fn view_named(key: &str) -> Option<View> {
    VIEW_KEYS.iter().find(|(_, k)| *k == key).map(|(v, _)| *v)
}

// ---- identity ---------------------------------------------------------------

/// FNV-1a over bytes — the one hash the store uses (log identity, fight
/// content ids, loadout addressing). Stable forever: it names files.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `<log:016x>-<start_ms>` (a pull) or `<log:016x>-<start_ms>s` (a visit's
/// Σ — a key or an Overall): the primary key of a fight. `log` is the fnv64
/// of the log's first complete line (its COMBAT_LOG_VERSION header, unique
/// per session) — or of the file name when the log began mid-session —
/// and `start_ms` is the segment's (or the visit's) start. Two pulls cannot
/// start on the same millisecond in one file, but a visit and its first
/// member can (an ENCOUNTER_START on the ZONE_CHANGE's millisecond), so
/// the Σ carries its own mark. A copy of the log (even CRLF-converted,
/// since the line is hashed without its ending) yields the same id.
pub fn fight_id(log: u64, start_ms: i64, sigma: bool) -> String {
    if sigma {
        format!("{log:016x}-{start_ms}s")
    } else {
        format!("{log:016x}-{start_ms}")
    }
}

/// The Σ spelling of an id: the mark appended unless already there. Stores
/// written before the mark existed filed Σ cards under the pull spelling;
/// `Store::open` renames them through this.
pub fn sigma_id(id: &str) -> String {
    if id.ends_with('s') {
        id.to_string()
    } else {
        format!("{id}s")
    }
}

/// The log-identity half of a fight id: hash the first complete line with
/// its line ending stripped, else the file name.
pub fn log_id(first_line: Option<&str>, file_name: &str) -> u64 {
    match first_line {
        Some(l) if !l.trim().is_empty() => fnv64(l.trim_end_matches(['\r', '\n']).as_bytes()),
        _ => fnv64(file_name.as_bytes()),
    }
}

/// Derived, not primary: the same pull seen from two people's logs shares
/// a content id but keeps separate records (their numbers differ). Exists
/// for export and annotation addressing. `guids` are the friendly players.
pub fn content_id(
    encounter: Option<Encounter>,
    start_utc_ms: i64,
    guids: impl IntoIterator<Item = impl AsRef<str>>,
) -> u64 {
    let mut names: Vec<String> = guids.into_iter().map(|g| g.as_ref().to_string()).collect();
    names.sort();
    names.dedup();
    let (id, diff, size) = encounter.map_or((0, 0, 0), |e| (e.id, e.difficulty, e.group_size));
    let canon = format!(
        "{id}|{diff}|{}|{size}|{}",
        start_utc_ms.div_euclid(1000),
        names.join(",")
    );
    fnv64(canon.as_bytes())
}

/// Content address of a loadout: fnv64 over its v19 wire encoding, so the
/// same build hashes the same off the socket and out of a file.
pub fn loadout_hash(l: &Loadout) -> u64 {
    fnv64(&crate::msg::loadout_bytes(l))
}

fn hex(h: u64) -> Json {
    Json::str(format!("{h:016x}"))
}

fn from_hex(v: Option<&Json>) -> Option<u64> {
    let s = v?.as_str()?;
    (s.len() == 16).then(|| u64::from_str_radix(s, 16).ok())?
}

// ---- the fight card ----------------------------------------------------------

/// What a stored fight is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FightKind {
    /// A raid boss (ENCOUNTER_START..END).
    Encounter,
    /// An arena match (R13).
    Arena,
    /// A keystone run's Overall (R10: the visit's Σ — what a key's history means).
    Key,
    /// An unkeyed instance visit's Overall.
    Overall,
    /// Out-of-encounter combat; stored only under `history_store_trash`.
    Trash,
}

impl FightKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FightKind::Encounter => "encounter",
            FightKind::Arena => "arena",
            FightKind::Key => "key",
            FightKind::Overall => "overall",
            FightKind::Trash => "trash",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "encounter" => FightKind::Encounter,
            "arena" => FightKind::Arena,
            "key" => FightKind::Key,
            "overall" => FightKind::Overall,
            "trash" => FightKind::Trash,
            _ => return None,
        })
    }
}

/// R10 facts of a keyed (or plain) instance visit, on `Key`/`Overall` cards.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyInfo {
    pub map_id: u32,
    pub difficulty: u32,
    /// The keystone level; `None` on an unkeyed visit.
    pub level: Option<u32>,
    /// CHALLENGE_MODE_END's success flag (the game's, not the timed verdict).
    pub completed: Option<bool>,
}

/// One player's line on a card: enough for every trend / best-per-player
/// query to run without opening the rows file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CardPlayer {
    pub guid: String,
    pub name: String,
    pub class: Option<Class>,
    pub spec: Option<Spec>,
    /// Content address of the player's COMBATANT_INFO loadout
    /// (`loadouts/<hash>.json`); `None` when the log never carried one.
    pub loadout: Option<u64>,
    /// A COMBATANT_INFO line named this player in this fight — the signal
    /// the owner inference intersects (spec §9).
    pub logged: bool,
    /// R13: fought on the hostile side of an arena match.
    pub enemy: bool,
    pub damage: u64,
    pub dps: f64,
    pub healing: u64,
    pub hps: f64,
    pub deaths: u32,
    /// R17 (step 2b): the player's Taken row amount — damage that reached
    /// them, absorbs included. 0 on a card written before step 2b.
    pub taken: u64,
    /// `Mitigation::mitigated` — partial absorbs + blocks + full absorbs +
    /// blocks. 0 on an older card.
    pub mitigated: u64,
    /// `Mitigation::prevented` — full blocks, the amount a miss carried
    /// that never became Taken. A card written before R1 counted a whole
    /// absorb holds its full absorbs here too, and its `taken` lacks them:
    /// `taken + prevented`, what `mitigated_pct` divides by, is the same
    /// total either way. 0 on an older card.
    pub prevented: u64,
    /// Damage taken per second over the R7 duration — the same path as
    /// `dps`. 0.0 on an older card.
    pub dtps: f64,
    /// Step 3b: the healing split — the Healing row's `extra` (overhealing)
    /// and the player's absorb healing (`Segment::absorbed_healing`), the
    /// healer's efficiency pair. 0 on a card written before step 3b.
    pub overheal: u64,
    pub absorbed: u64,
    /// R19: damage shares this player GAVE as a supporter (an Augmentation
    /// Evoker's `_SUPPORT` lines credited to others) and RECEIVED from
    /// supporters — the two scalars `effective` folds against `damage`.
    /// Healing shares stay on the rows tier (`FightRows::support`). 0 on
    /// an older card.
    pub support_given: u64,
    pub support_received: u64,
    /// Healing this player received from others and healed on themselves
    /// (`Segment::healed`): the tank pair beside `taken`. 0 on an older
    /// card.
    pub healed_received: u64,
    pub self_healed: u64,
    /// R18 (step 4b): the per-millisecond UNION of the player's
    /// `ActiveMitigation` spans (`Segment::am_uptime_ms`, clamped at the
    /// R7 clock, so never over `duration_ms`); `am_uptime_pct` is DERIVED
    /// from it and the card's duration, never stored twice. 0 on a card
    /// written before step 4b.
    pub am_uptime_ms: u64,
    /// R18: externals the player GAVE (`External` spans they cast on
    /// others) and RECEIVED, as a count of spans and their total ms
    /// (`Segment::externals_given` / `externals_received`). 0 on an
    /// older card.
    pub externals_given: u32,
    pub externals_given_ms: u64,
    pub externals_received: u32,
    pub externals_received_ms: u64,
    /// R20 (step 5): Σ `wasted` over the player's closed shields whose
    /// waste was KNOWN (`Segment::absorb_wasted`) — `None` when no shield
    /// of theirs closed with a known waste (a non-shielder, or an
    /// un-regraded pre-5 card, which reads `None` for the missing key
    /// and for `null` alike). `absorb_efficiency` is DERIVED from it and
    /// `absorbed`, never stored twice.
    pub absorb_wasted: Option<u64>,
    /// R20: how many of the player's shields had an unknown APPLIED
    /// amount — the pre-pull ones and those still open at the close
    /// (`Segment::shields_unknown`); the healer block's caveat. 0 on an
    /// older card.
    pub shields_unknown: u32,
    /// R17 amendment (v43): what armor and damage reduction took off the
    /// hits on the player (`Mitigation::reduced`) — already inside
    /// `mitigated`, and added to the swung total `mitigated_pct` divides
    /// by. 0 on a card written before it (its `mitigated` lacks it too, so
    /// its pct is the one it always had).
    pub reduced: u64,
    /// R2 (v44): the part of `healing` a heal-absorb ate
    /// (`Segment::heal_absorbed`) — healing done all the same, the third
    /// half of the healing split beside `overheal` and `absorbed`. 0 on a
    /// card written before it.
    pub heal_absorbed: u64,
    /// v31: the player's guild as the wowdps addon last saw them — joined
    /// from `affiliations/` when a card is ANSWERED, never stored on it
    /// (`to_json` skips it, `from_json` reads `None`): the addon's file
    /// lands after the night, so a stored value would be wrong on every
    /// card written before it. `None` = unknown, `Some("")` = seen without
    /// a guild.
    pub guild: Option<String>,
}

/// `fights/<id>.json` — ~400 B plus ~90 B per player, always written. The
/// daemon's in-memory index is a `Vec` of these.
#[derive(Debug, Clone, PartialEq)]
pub struct FightCard {
    pub schema: u16,
    pub id: String,
    /// The log-identity half of `id`.
    pub log: u64,
    pub content: u64,
    pub kind: FightKind,
    pub name: String,
    pub encounter: Option<Encounter>,
    pub key: Option<KeyInfo>,
    /// The segment's `start_ms`: a LOCAL-time epoch as the log wrote it.
    pub start_local_ms: i64,
    /// The log's timezone offset (minutes east of UTC); `None` on a legacy
    /// log without one — then `start_utc_ms == start_local_ms`, flagged.
    pub tz_min: Option<i16>,
    pub start_utc_ms: i64,
    /// R7 semantics (a key: the key clock).
    pub duration_ms: i64,
    /// v40 (R7/R10 amendment): the COMBAT clock the rows' rates ran on —
    /// `Segment::combat_ms`. `None` on a card written before it, whose rates
    /// ran on `duration_ms`; [`Self::rate_ms`] is the one reader.
    pub combat_ms: Option<i64>,
    /// Keys: CHALLENGE_MODE_END's totalMs.
    pub official_ms: Option<i64>,
    /// Keys: the dungeon's (par, +2, +3) timers.
    pub pars_ms: Option<(i64, i64, i64)>,
    /// Kill / wipe, win / loss, timed / depleted. `None` while aborted.
    pub success: Option<bool>,
    /// Closed by a version seam, a rotation or a daemon exit rather than
    /// its END: listed, never counted as a pull.
    pub aborted: bool,
    pub build: (u16, u16, u16),
    pub project_id: u8,
    pub log_version: u32,
    /// The logger's guid as configured or inferred at write time.
    pub owner: Option<String>,
    /// Provenance when the index had it: the slice's `[start, end)` offsets.
    pub byte_range: Option<(u64, u64)>,
    /// Protected from retention; the one field a card is rewritten for.
    pub pinned: bool,
    /// R16's lowest observed boss health, whole percent rounded down: 0 on a kill,
    /// the lowest the boss was seen at on a wipe, `None` when nothing said
    /// (no health report, or not a boss pull at all). Written from
    /// `Segment::best_pct` on every stored Encounter.
    pub best_pct: Option<u16>,
    pub players: Vec<CardPlayer>,
    /// Keys only: the member bosses the Σ merged, in pull order — what a
    /// reader can drill into with `GetFight { boss }` (parsed from the log
    /// on demand; members are not stored on their own).
    pub bosses: Vec<KeyBoss>,
}

/// One boss pull inside a keystone run, as the key's card lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyBoss {
    pub name: String,
    pub encounter: Option<Encounter>,
    pub start_utc_ms: i64,
    pub duration_ms: i64,
    pub success: Option<bool>,
}

impl KeyBoss {
    pub fn to_json(&self) -> Json {
        obj! {
            "name": Json::str(self.name.clone()),
            "encounter": self.encounter.map_or(Json::Null, encounter_json),
            "start_utc_ms": Json::num(self.start_utc_ms as f64),
            "duration_ms": Json::num(self.duration_ms as f64),
            "success": self.success.map_or(Json::Null, Json::Bool),
        }
    }

    pub fn from_json(v: &Json) -> Option<Self> {
        Some(Self {
            name: str_of(v, "name")?.to_string(),
            encounter: v.get("encounter").and_then(encounter_from),
            start_utc_ms: i64_of(v, "start_utc_ms").unwrap_or(0),
            duration_ms: i64_of(v, "duration_ms").unwrap_or(0),
            success: bool_of(v, "success"),
        })
    }
}

impl Default for FightCard {
    fn default() -> Self {
        Self {
            schema: HISTORY_SCHEMA,
            id: String::new(),
            log: 0,
            content: 0,
            kind: FightKind::Encounter,
            name: String::new(),
            encounter: None,
            key: None,
            start_local_ms: 0,
            tz_min: None,
            start_utc_ms: 0,
            duration_ms: 0,
            combat_ms: None,
            official_ms: None,
            pars_ms: None,
            success: None,
            aborted: false,
            build: (0, 0, 0),
            project_id: 0,
            log_version: 0,
            owner: None,
            byte_range: None,
            pinned: false,
            best_pct: None,
            players: Vec::new(),
            bosses: Vec::new(),
        }
    }
}

impl FightCard {
    /// The clock the card's rates run on: `combat_ms`, or `duration_ms` on a
    /// card written before v40 — so an older card reads exactly as it did.
    /// A keystone run's RUN rate is `wowdps_model::rate(amount, duration_ms)`.
    pub fn rate_ms(&self) -> i64 {
        self.combat_ms.unwrap_or(self.duration_ms)
    }

    pub fn to_json(&self) -> Json {
        obj! {
            "schema": Json::num(self.schema),
            "id": Json::str(&*self.id),
            "log": hex(self.log),
            "content": hex(self.content),
            "kind": Json::str(self.kind.as_str()),
            "name": Json::str(&*self.name),
            "encounter": self.encounter.map_or(Json::Null, encounter_json),
            "key": self.key.as_ref().map_or(Json::Null, |k| obj! {
                "map_id": Json::num(k.map_id),
                "difficulty": Json::num(k.difficulty),
                "level": opt_num(k.level.map(u64::from)),
                "completed": opt_bool(k.completed),
            }),
            "start_local_ms": Json::num(self.start_local_ms as f64),
            "tz_min": self.tz_min.map_or(Json::Null, Json::num),
            "start_utc_ms": Json::num(self.start_utc_ms as f64),
            "duration_ms": Json::num(self.duration_ms as f64),
            "combat_ms": self.combat_ms.map_or(Json::Null, |m| Json::num(m as f64)),
            "official_ms": self.official_ms.map_or(Json::Null, |m| Json::num(m as f64)),
            "pars_ms": pars_json(self.pars_ms),
            "success": opt_bool(self.success),
            "aborted": Json::Bool(self.aborted),
            "build": Json::str(format!("{}.{}.{}", self.build.0, self.build.1, self.build.2)),
            "project_id": Json::num(self.project_id),
            "log_version": Json::num(self.log_version),
            "owner": self.owner.as_deref().map_or(Json::Null, Json::str),
            "byte_range": self.byte_range.map_or(Json::Null, |(a, b)| {
                Json::Arr(vec![Json::u64(a), Json::u64(b)])
            }),
            "pinned": Json::Bool(self.pinned),
            "best_pct": opt_num(self.best_pct.map(u64::from)),
            "players": Json::Arr(
                self.players
                    .iter()
                    .map(|p| p.to_json_in(Some((self.duration_ms, self.rate_ms()))))
                    .collect()
            ),
            "bosses": Json::Arr(self.bosses.iter().map(KeyBoss::to_json).collect()),
        }
    }

    /// `None` only without an identity (`schema` and `id`); everything else
    /// defaults, so older `v1` documents read after fields are added.
    pub fn from_json(v: &Json) -> Option<Self> {
        let (schema, id) = identity(v)?;
        let d = Self::default();
        Some(Self {
            schema,
            id,
            log: from_hex(v.get("log")).unwrap_or(0),
            content: from_hex(v.get("content")).unwrap_or(0),
            kind: str_of(v, "kind")
                .and_then(FightKind::parse)
                .unwrap_or(FightKind::Encounter),
            name: str_of(v, "name").unwrap_or_default().to_string(),
            encounter: v.get("encounter").and_then(encounter_from),
            key: v.get("key").and_then(|k| {
                matches!(k, Json::Obj(_)).then(|| KeyInfo {
                    map_id: u32_of(k, "map_id").unwrap_or(0),
                    difficulty: u32_of(k, "difficulty").unwrap_or(0),
                    level: u32_of(k, "level"),
                    completed: bool_of(k, "completed"),
                })
            }),
            start_local_ms: i64_of(v, "start_local_ms").unwrap_or(0),
            tz_min: i64_of(v, "tz_min").and_then(|m| i16::try_from(m).ok()),
            start_utc_ms: i64_of(v, "start_utc_ms").unwrap_or(0),
            duration_ms: i64_of(v, "duration_ms").unwrap_or(0),
            combat_ms: i64_of(v, "combat_ms"),
            official_ms: i64_of(v, "official_ms"),
            pars_ms: pars_from(v.get("pars_ms")),
            success: bool_of(v, "success"),
            aborted: bool_of(v, "aborted").unwrap_or(false),
            build: str_of(v, "build").map_or(d.build, parse_build),
            project_id: u32_of(v, "project_id")
                .and_then(|p| u8::try_from(p).ok())
                .unwrap_or(0),
            log_version: u32_of(v, "log_version").unwrap_or(0),
            owner: str_of(v, "owner").map(str::to_string),
            byte_range: v.get("byte_range").and_then(|r| {
                let a = r.as_arr()?;
                Some((a.first()?.as_u64()?, a.get(1)?.as_u64()?))
            }),
            pinned: bool_of(v, "pinned").unwrap_or(false),
            best_pct: u32_of(v, "best_pct").and_then(|p| u16::try_from(p).ok()),
            players: v
                .get("players")
                .and_then(Json::as_arr)
                .map(|a| a.iter().filter_map(CardPlayer::from_json).collect())
                .unwrap_or_default(),
            bosses: v
                .get("bosses")
                .and_then(Json::as_arr)
                .map(|a| a.iter().filter_map(KeyBoss::from_json).collect())
                .unwrap_or_default(),
        })
    }
}

/// Friendly players per role on a card, from `CardPlayer::role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoleCount {
    pub tanks: u32,
    pub healers: u32,
    pub dps: u32,
}

impl CardPlayer {
    /// The role the spec plays (roadmap item 1a, step 1). Derived, never
    /// stored in memory: the spec is the truth. `to_json` writes it as
    /// `role` for readers that cannot call `Spec::role` (DuckDB); `from_json`
    /// ignores the field.
    pub fn role(&self) -> Option<Role> {
        self.spec.map(Spec::role)
    }

    /// R17: `mitigated / (taken + prevented + reduced)` × 100 through the
    /// model's one [`wowdps_model::mitigated_pct`]. Derived the way `role` is:
    /// never a struct field, written to JSON as `mitigated_pct` for readers
    /// that cannot do the arithmetic themselves (DuckDB), ignored on read.
    /// 0.0 on a card without the tank measures.
    pub fn mitigated_pct(&self) -> f64 {
        wowdps_model::mitigated_pct(self.mitigated, self.taken, self.prevented, self.reduced)
    }

    /// R18 (step 4b): the player's active-mitigation uptime as a
    /// percentage of the card's `duration_ms` — `am_uptime_ms × 100 /
    /// duration_ms`, the union over the R7 clock, so it never exceeds 100
    /// on a card the engine wrote. 0.0 when the duration is not positive
    /// (an aborted card). Derived like `effective_dps`: written to JSON as
    /// `am_uptime_pct` beside `mitigated_pct` for readers that cannot do
    /// the arithmetic (DuckDB), ignored on read. 0.0 on a pre-4b card.
    pub fn am_uptime_pct(&self, duration_ms: i64) -> f64 {
        if duration_ms > 0 {
            self.am_uptime_ms as f64 * 100.0 / duration_ms as f64
        } else {
            0.0
        }
    }

    /// R19 (step 3b): the player's effective damage — `damage` minus the
    /// shares supporters gave them plus the shares they gave others —
    /// through the model's one [`wowdps_model::effective`] (clamped at 0,
    /// never a wrap). Equal to `damage` on a card without support scalars,
    /// so an older card's effective is its raw damage.
    pub fn effective(&self) -> u64 {
        wowdps_model::effective(self.damage, self.support_received, self.support_given)
    }

    /// Effective damage per second over a clock — pass the card's
    /// [`FightCard::rate_ms`], the clock its rows' rates ran on — the SAME
    /// arithmetic `Meter::finish_rows` uses for a rate row's `per_sec`
    /// (`amount as f64 / secs` with `secs = clock as f64 / 1000.0`), so on
    /// a fight without support it is `dps` bit for bit, which is what lets
    /// grading and trend rank it with no predicate.
    /// 0.0 when the duration is not positive (an aborted card), as a rate
    /// row would be. Derived: written to JSON as `effective_dps` for
    /// readers that cannot do the fold (DuckDB), ignored on read.
    pub fn effective_dps(&self, duration_ms: i64) -> f64 {
        let secs = duration_ms as f64 / 1000.0;
        if secs > 0.0 {
            self.effective() as f64 / secs
        } else {
            0.0
        }
    }
}

impl FightCard {
    /// Role head-count over the friendly side; players whose spec is
    /// unknown (R8 inference failed) count nowhere.
    pub fn roles(&self) -> RoleCount {
        let mut out = RoleCount::default();
        for p in self.players.iter().filter(|p| !p.enemy) {
            match p.role() {
                Some(Role::Tank) => out.tanks += 1,
                Some(Role::Healer) => out.healers += 1,
                Some(Role::Dps) => out.dps += 1,
                None => {}
            }
        }
        out
    }
}

impl CardPlayer {
    /// The player's line without its card: `effective_dps` needs the
    /// card's duration, so here it is written `null`. `FightCard::to_json`
    /// goes through [`CardPlayer::to_json_in`] and writes the number.
    pub fn to_json(&self) -> Json {
        self.to_json_in(None)
    }

    /// The player's line inside a card of `(duration_ms, rate_ms)`:
    /// `am_uptime_pct` is derived over the first (the R7 clock its spans
    /// close on), `effective_dps` over the second (the card's rate clock,
    /// [`FightCard::rate_ms`]); `None` writes `null` for both.
    pub fn to_json_in(&self, clocks: Option<(i64, i64)>) -> Json {
        let duration_ms = clocks.map(|c| c.0);
        let rate_ms = clocks.map(|c| c.1);
        obj! {
            "guid": Json::str(&*self.guid),
            "name": Json::str(&*self.name),
            "class": self.class.map_or(Json::Null, |c| Json::str(class_name(c))),
            "spec": opt_num(self.spec.map(|s| u64::from(s.id()))),
            "spec_name": self.spec.map_or(Json::Null, |s| Json::str(s.name())),
            "role": self.role().map_or(Json::Null, |r| Json::str(r.name())),
            "loadout": self.loadout.map_or(Json::Null, hex),
            "logged": Json::Bool(self.logged),
            "enemy": Json::Bool(self.enemy),
            "damage": Json::u64(self.damage),
            "dps": Json::num(self.dps),
            "healing": Json::u64(self.healing),
            "hps": Json::num(self.hps),
            "deaths": Json::num(self.deaths),
            "taken": Json::u64(self.taken),
            "mitigated": Json::u64(self.mitigated),
            "prevented": Json::u64(self.prevented),
            "dtps": Json::num(self.dtps),
            "mitigated_pct": Json::num(self.mitigated_pct()),
            // Step 4b: derived from `am_uptime_ms` and the card's duration,
            // `null` without a card, like `effective_dps`.
            "am_uptime_pct": duration_ms.map_or(Json::Null, |d| Json::num(self.am_uptime_pct(d))),
            // Step 5: derived from `absorbed` and `absorb_wasted`, `null`
            // when the waste is unknown — never 0, which would read as a
            // fully wasted shielder.
            "absorb_efficiency": self.absorb_efficiency().map_or(Json::Null, Json::num),
            "overheal": Json::u64(self.overheal),
            "absorbed": Json::u64(self.absorbed),
            "support_given": Json::u64(self.support_given),
            "support_received": Json::u64(self.support_received),
            "healed_received": Json::u64(self.healed_received),
            "self_healed": Json::u64(self.self_healed),
            "am_uptime_ms": Json::u64(self.am_uptime_ms),
            "externals_given": Json::num(self.externals_given),
            "externals_given_ms": Json::u64(self.externals_given_ms),
            "externals_received": Json::num(self.externals_received),
            "externals_received_ms": Json::u64(self.externals_received_ms),
            "effective_dps": rate_ms.map_or(Json::Null, |d| Json::num(self.effective_dps(d))),
            // Step 5 (R20): `null` when unknown, so SQL's NULL is honest.
            "absorb_wasted": self.absorb_wasted.map_or(Json::Null, Json::u64),
            "shields_unknown": Json::num(self.shields_unknown),
            // v43 (R17 amendment), trailing so every older line's prefix stands.
            "reduced": Json::u64(self.reduced),
            // v44 (R2): what a heal-absorb ate of the healing, trailing.
            "heal_absorbed": Json::u64(self.heal_absorbed),
        }
    }

    pub fn from_json(v: &Json) -> Option<Self> {
        let guid = str_of(v, "guid")?.to_string();
        Some(Self {
            guid,
            name: str_of(v, "name").unwrap_or_default().to_string(),
            class: str_of(v, "class").and_then(class_from_name),
            spec: u32_of(v, "spec").and_then(Spec::from_id),
            loadout: from_hex(v.get("loadout")),
            logged: bool_of(v, "logged").unwrap_or(false),
            enemy: bool_of(v, "enemy").unwrap_or(false),
            damage: u64_of(v, "damage").unwrap_or(0),
            dps: f64_of(v, "dps").unwrap_or(0.0),
            healing: u64_of(v, "healing").unwrap_or(0),
            hps: f64_of(v, "hps").unwrap_or(0.0),
            deaths: u32_of(v, "deaths").unwrap_or(0),
            // Step 2b's tank measures; a PR #16 card has none. `mitigated_pct`
            // is derived and deliberately not read back (see `mitigated_pct`).
            taken: u64_of(v, "taken").unwrap_or(0),
            mitigated: u64_of(v, "mitigated").unwrap_or(0),
            prevented: u64_of(v, "prevented").unwrap_or(0),
            dtps: f64_of(v, "dtps").unwrap_or(0.0),
            // Step 3b's healing split and support scalars; a PR #19 card
            // has none. `effective_dps` is derived (`effective_dps`) and
            // deliberately not read back — a stored value that lies is
            // re-derived on the next write.
            overheal: u64_of(v, "overheal").unwrap_or(0),
            absorbed: u64_of(v, "absorbed").unwrap_or(0),
            support_given: u64_of(v, "support_given").unwrap_or(0),
            support_received: u64_of(v, "support_received").unwrap_or(0),
            healed_received: u64_of(v, "healed_received").unwrap_or(0),
            self_healed: u64_of(v, "self_healed").unwrap_or(0),
            // Step 4b's aura-span scalars; a pre-4b card has none and
            // reads zeros. `am_uptime_pct` is derived and not read back.
            am_uptime_ms: u64_of(v, "am_uptime_ms").unwrap_or(0),
            externals_given: u32_of(v, "externals_given").unwrap_or(0),
            externals_given_ms: u64_of(v, "externals_given_ms").unwrap_or(0),
            externals_received: u32_of(v, "externals_received").unwrap_or(0),
            externals_received_ms: u64_of(v, "externals_received_ms").unwrap_or(0),
            // Step 5 (R20): a missing key (pre-5 card) and `null` both read
            // `None`; `absorb_efficiency` is derived and not read back.
            absorb_wasted: u64_of(v, "absorb_wasted"),
            shields_unknown: u32_of(v, "shields_unknown").unwrap_or(0),
            // v43: armor's share; a card written before it has none.
            reduced: u64_of(v, "reduced").unwrap_or(0),
            // v44 (R2): a card written before it has none.
            heal_absorbed: u64_of(v, "heal_absorbed").unwrap_or(0),
            // v31: never stored; the store joins it when it answers.
            guild: None,
        })
    }

    /// R20 (step 5): the player's absorb efficiency — the ratio
    /// `absorbed / (absorbed + absorb_wasted)` — `Some` only when the waste
    /// is known and the sum is positive (a shielder whose shields all closed known-empty and
    /// absorbed nothing has no ratio). Derived like `am_uptime_pct`:
    /// written to JSON as `absorb_efficiency` (`null` when `None`) for
    /// readers that cannot do the arithmetic, ignored on read. Open
    /// shields at the close count nothing on either side.
    pub fn absorb_efficiency(&self) -> Option<f64> {
        let wasted = self.absorb_wasted?;
        let total = self.absorbed.checked_add(wasted)?;
        (total > 0).then(|| self.absorbed as f64 / total as f64)
    }
}

/// One player's shield ledger on the rows tier (R20, step 5): their
/// `Segment::shields` rows — one per spell they cast a shield of, consumed
/// desc. Friendly players with any row only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerShields {
    pub guid: String,
    pub rows: Vec<ShieldRow>,
}

/// One ledger row as the rows tier writes it.
pub fn shield_row_json(r: &ShieldRow) -> Json {
    obj! {
        "spell_id": Json::num(r.spell_id),
        "label": Json::str(&*r.label),
        "applied": Json::u64(r.applied),
        "consumed": Json::u64(r.consumed),
        "wasted": Json::u64(r.wasted),
        "count": Json::num(r.count),
        "unknown": Json::num(r.unknown),
    }
}

/// `None` without a spell id (the row is dropped, not the block).
pub fn shield_row_from(v: &Json) -> Option<ShieldRow> {
    Some(ShieldRow {
        spell_id: u32_of(v, "spell_id")?,
        label: str_of(v, "label").unwrap_or_default().to_string(),
        applied: u64_of(v, "applied").unwrap_or(0),
        consumed: u64_of(v, "consumed").unwrap_or(0),
        wasted: u64_of(v, "wasted").unwrap_or(0),
        count: u32_of(v, "count").unwrap_or(0),
        unknown: u32_of(v, "unknown").unwrap_or(0),
    })
}

impl PlayerShields {
    pub fn to_json(&self) -> Json {
        obj! {
            "guid": Json::str(&*self.guid),
            "rows": Json::Arr(self.rows.iter().map(shield_row_json).collect()),
        }
    }

    /// `None` without a guid; a malformed row list reads empty.
    pub fn from_json(v: &Json) -> Option<Self> {
        Some(Self {
            guid: str_of(v, "guid")?.to_string(),
            rows: v
                .get("rows")
                .and_then(Json::as_arr)
                .map(|a| a.iter().filter_map(shield_row_from).collect())
                .unwrap_or_default(),
        })
    }
}

/// One player's stack ledger on the rows tier (R21, step 6): the hostile
/// debuffs seen open on them and the raw per-level cells — never the
/// derived level 0, so SQL derives it the same way the daemon does.
/// Friendly players only, and only those with something to say: a cell, a
/// debuff seen, a dropped hit, or a `base` entry — the last means EVERY
/// player who took a hit or a miss gets a block, because `base` is also the
/// only per-spell-ID taken baseline the rows tier carries (`taken_spells` is
/// per NAME). `debuffs` and `cells` are then empty for them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerStacks {
    pub guid: String,
    pub dropped: u32,
    pub debuffs: Vec<StackingDebuff>,
    pub cells: Vec<StackCell>,
    /// The unconditioned baseline per damage spell id (retest 21).
    pub base: Vec<StackBase>,
}

pub fn stack_base_json(b: &StackBase) -> Json {
    obj! {
        "damage_spell_id": Json::num(b.damage_spell_id),
        "damage_label": Json::str(&*b.damage_label),
        "hits": Json::num(b.hits),
        "sum": Json::u64(b.sum),
        "misses": Json::num(b.misses),
    }
}

/// `None` without a label (the entry is dropped, not the block).
pub fn stack_base_from(v: &Json) -> Option<StackBase> {
    Some(StackBase {
        damage_spell_id: u32_of(v, "damage_spell_id").unwrap_or(0),
        damage_label: str_of(v, "damage_label")?.to_string(),
        hits: u32_of(v, "hits").unwrap_or(0),
        sum: u64_of(v, "sum").unwrap_or(0),
        misses: u32_of(v, "misses").unwrap_or(0),
    })
}

pub fn stacking_debuff_json(d: &StackingDebuff) -> Json {
    obj! {
        "spell_id": Json::num(d.spell_id),
        "label": Json::str(&*d.label),
        "src": Json::str(&*d.src),
        "max_level": Json::num(d.max_level),
        "hits": Json::num(d.hits),
    }
}

/// `None` without a spell id (the entry is dropped, not the block).
pub fn stacking_debuff_from(v: &Json) -> Option<StackingDebuff> {
    Some(StackingDebuff {
        spell_id: u32_of(v, "spell_id")?,
        label: str_of(v, "label").unwrap_or_default().to_string(),
        src: str_of(v, "src").unwrap_or_default().to_string(),
        max_level: u32_of(v, "max_level").unwrap_or(0).min(u16::MAX as u32) as u16,
        hits: u32_of(v, "hits").unwrap_or(0),
    })
}

pub fn stack_cell_json(c: &StackCell) -> Json {
    obj! {
        "damage_spell_id": Json::num(c.damage_spell_id),
        "damage_label": Json::str(&*c.damage_label),
        "aura_spell_id": Json::num(c.aura_spell_id),
        "level": Json::num(c.level),
        "hits": Json::num(c.hits),
        "sum": Json::u64(c.sum),
        "max": Json::u64(c.max),
    }
}

/// `None` without an aura id or a level (the cell is dropped, not the block).
pub fn stack_cell_from(v: &Json) -> Option<StackCell> {
    Some(StackCell {
        damage_spell_id: u32_of(v, "damage_spell_id").unwrap_or(0),
        damage_label: str_of(v, "damage_label").unwrap_or_default().to_string(),
        aura_spell_id: u32_of(v, "aura_spell_id")?,
        level: u32_of(v, "level")?.min(u16::MAX as u32) as u16,
        hits: u32_of(v, "hits").unwrap_or(0),
        sum: u64_of(v, "sum").unwrap_or(0),
        max: u64_of(v, "max").unwrap_or(0),
    })
}

impl PlayerStacks {
    pub fn to_json(&self) -> Json {
        obj! {
            "guid": Json::str(&*self.guid),
            "dropped": Json::num(self.dropped),
            "debuffs": Json::Arr(self.debuffs.iter().map(stacking_debuff_json).collect()),
            "cells": Json::Arr(self.cells.iter().map(stack_cell_json).collect()),
            "base": Json::Arr(self.base.iter().map(stack_base_json).collect()),
        }
    }

    /// `None` without a guid; malformed lists read empty.
    pub fn from_json(v: &Json) -> Option<Self> {
        let list = |key: &str| v.get(key).and_then(Json::as_arr);
        Some(Self {
            guid: str_of(v, "guid")?.to_string(),
            dropped: u32_of(v, "dropped").unwrap_or(0),
            debuffs: list("debuffs")
                .map(|a| a.iter().filter_map(stacking_debuff_from).collect())
                .unwrap_or_default(),
            cells: list("cells")
                .map(|a| a.iter().filter_map(stack_cell_from).collect())
                .unwrap_or_default(),
            base: list("base")
                .map(|a| a.iter().filter_map(stack_base_from).collect())
                .unwrap_or_default(),
        })
    }
}

/// One player's aura-uptime rollup on the rows tier (R18, step 4b): the
/// `Segment::uptime` cells keyed by TARGET — the player is the buffed one,
/// each cell's `src` is who cast it — uncapped. A supporter's per-target
/// uptime and "externals given, to whom" are derived from OTHER players'
/// cells by `src`, so nothing is stored twice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerUptime {
    pub guid: String,
    pub cells: Vec<UptimeCell>,
}

/// One cell as the rows tier writes it: `kind` is the NAME
/// (`MarkKind::name`, `"external"` …) so SQL can say `kind = 'external'`;
/// details' timeline marks keep the code.
pub fn uptime_cell_json(c: &UptimeCell) -> Json {
    obj! {
        "spell_id": Json::num(c.spell_id),
        "label": Json::str(&*c.label),
        "kind": Json::str(c.kind.name()),
        "src": Json::str(&*c.src),
        "count": Json::num(c.count),
        "total_ms": Json::num(c.total_ms as f64),
    }
}

/// `None` on an unknown kind name (the cell is dropped, not the block).
pub fn uptime_cell_from(v: &Json) -> Option<UptimeCell> {
    Some(UptimeCell {
        spell_id: u32_of(v, "spell_id").unwrap_or(0),
        label: str_of(v, "label").unwrap_or_default().to_string(),
        kind: str_of(v, "kind").and_then(MarkKind::from_name)?,
        src: str_of(v, "src").unwrap_or_default().to_string(),
        count: u32_of(v, "count").unwrap_or(0),
        total_ms: i64_of(v, "total_ms").unwrap_or(0),
    })
}

impl PlayerUptime {
    pub fn to_json(&self) -> Json {
        obj! {
            "guid": Json::str(&*self.guid),
            "cells": Json::Arr(self.cells.iter().map(uptime_cell_json).collect()),
        }
    }

    /// `None` without a guid; a malformed cell list reads empty.
    pub fn from_json(v: &Json) -> Option<Self> {
        Some(Self {
            guid: str_of(v, "guid")?.to_string(),
            cells: v
                .get("cells")
                .and_then(Json::as_arr)
                .map(|a| a.iter().filter_map(uptime_cell_from).collect())
                .unwrap_or_default(),
        })
    }
}

/// One player's coarse series on the rows tier (R18, step 4b): the taken
/// and healing timelines coarsened to 10 s buckets (`bucket_ms` is fixed
/// at [`COARSE_BUCKET_MS`], not stored) and the ONE merged mark list —
/// item marks and role spans, `Mark.kind` telling them apart — that every
/// drill's marks are. Friendly players only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerCoarse {
    pub guid: String,
    pub taken10: Vec<u64>,
    pub heal10: Vec<u64>,
    pub marks: Vec<Mark>,
}

/// The coarse series' bucket width: `Timeline::coarsen(10)` over the
/// engine's 1 s grid.
pub const COARSE_BUCKET_MS: u32 = 10_000;

impl PlayerCoarse {
    pub fn to_json(&self) -> Json {
        obj! {
            "guid": Json::str(&*self.guid),
            "taken10": Json::Arr(self.taken10.iter().map(|b| Json::u64(*b)).collect()),
            "heal10": Json::Arr(self.heal10.iter().map(|b| Json::u64(*b)).collect()),
            "marks": Json::Arr(self.marks.iter().map(mark_json).collect()),
        }
    }

    /// `None` without a guid; a malformed list reads empty.
    pub fn from_json(v: &Json) -> Option<Self> {
        let u64s = |key: &str| {
            v.get(key)
                .and_then(Json::as_arr)
                .map(|a| a.iter().filter_map(Json::as_u64).collect())
                .unwrap_or_default()
        };
        Some(Self {
            guid: str_of(v, "guid")?.to_string(),
            taken10: u64s("taken10"),
            heal10: u64s("heal10"),
            marks: marks_from(v.get("marks")),
        })
    }

    /// The taken series as a drill's `Timeline` (the marks cloned).
    pub fn taken_timeline(&self) -> Timeline {
        Timeline {
            bucket_ms: COARSE_BUCKET_MS,
            buckets: self.taken10.clone(),
            marks: self.marks.clone(),
        }
    }

    /// The healing series as a drill's `Timeline` (the marks cloned).
    pub fn heal_timeline(&self) -> Timeline {
        Timeline {
            bucket_ms: COARSE_BUCKET_MS,
            buckets: self.heal10.clone(),
            marks: self.marks.clone(),
        }
    }
}

/// One supporter's block on the rows tier (R19, step 3b): the shares they
/// gave and received, split damage / healing, and their per-target table
/// — `Segment::support_targets` verbatim (key = buffed owner guid,
/// `amount` = damage shares, `extra` = healing shares, `count` = lines).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerSupport {
    pub guid: String,
    pub given_damage: u64,
    pub given_healing: u64,
    pub received_damage: u64,
    pub received_healing: u64,
    pub targets: Vec<Row>,
}

impl PlayerSupport {
    pub fn to_json(&self) -> Json {
        obj! {
            "guid": Json::str(&*self.guid),
            "given": obj! {
                "damage": Json::u64(self.given_damage),
                "healing": Json::u64(self.given_healing),
            },
            "received": obj! {
                "damage": Json::u64(self.received_damage),
                "healing": Json::u64(self.received_healing),
            },
            "targets": rows_json(&self.targets),
        }
    }

    /// `None` without a guid; a malformed side reads as zeros.
    pub fn from_json(v: &Json) -> Option<Self> {
        let guid = str_of(v, "guid")?.to_string();
        let side = |key: &str| {
            let s = v.get(key);
            (
                s.and_then(|s| u64_of(s, "damage")).unwrap_or(0),
                s.and_then(|s| u64_of(s, "healing")).unwrap_or(0),
            )
        };
        let (given_damage, given_healing) = side("given");
        let (received_damage, received_healing) = side("received");
        Some(Self {
            guid,
            given_damage,
            given_healing,
            received_damage,
            received_healing,
            targets: rows_from(v.get("targets")),
        })
    }
}

// ---- rows and details ----------------------------------------------------------

/// One player's death recap (R9): the recap timeline newest-first and the
/// attacker totals — `Segment::breakdown(guid, View::Deaths)` verbatim.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Recap {
    pub guid: String,
    pub events: Vec<Row>,
    pub attackers: Vec<Row>,
    /// v28 (R9): which of the player's deaths this window is, oldest first —
    /// a player who died three times writes three `Recap`s under one guid.
    /// A record written before v28 has one window and reads back as index 0.
    pub index: u32,
    /// v28: the death's moment, ms from the fight's start.
    pub at_ms: i64,
    /// v28: windows the meter's per-player cap turned away, repeated on
    /// every window of that player so any one of them reconciles.
    pub dropped: u32,
}

/// R17 (step 2b): how many of a player's taken-by-ability rows the rows
/// tier keeps — the top N by amount; the rest fold into `TakenOther`. The
/// fold itself is the daemon's job (`extract()`); this module only fixes
/// the number so every writer agrees. The same cap bounds `taken_sources`
/// (its fold is `other_sources`). A boss pull has ~9 abilities and ~5
/// attackers, so the cap mostly bites Σ records — keys / overalls with
/// 60+ abilities and every NPC name in the dungeon as an attacker.
pub const TAKEN_SPELLS_CAP: usize = 16;

/// The rolled-up remainder of a capped `taken_spells` list — a struct, not
/// a fake `Row` (a `Row` with `spell_id` 0 and an empty key would collide
/// with Melee and double count in SQL). `n` is how many abilities were
/// folded; `n > 0` tells a reader the list was capped. Identity: Σ
/// `taken_spells.amount` + `other.amount` = the player's Taken row amount.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TakenOther {
    pub amount: u64,
    /// Σ the folded rows' `extra` (absorbed).
    pub extra: u64,
    /// Σ the folded rows' `count` (hits + misses).
    pub count: u64,
    /// Abilities folded.
    pub n: u32,
}

impl TakenOther {
    pub fn to_json(&self) -> Json {
        obj! {
            "amount": Json::u64(self.amount),
            "extra": Json::u64(self.extra),
            "count": Json::u64(self.count),
            "n": Json::num(self.n),
        }
    }

    /// A missing or malformed object reads as the empty remainder.
    pub fn from_json(v: Option<&Json>) -> Self {
        let Some(v) = v else {
            return Self::default();
        };
        Self {
            amount: u64_of(v, "amount").unwrap_or(0),
            extra: u64_of(v, "extra").unwrap_or(0),
            count: u64_of(v, "count").unwrap_or(0),
            n: u32_of(v, "n").unwrap_or(0),
        }
    }
}

/// R17 (step 2b): one player's mitigation on the rows tier — the
/// `Mitigation` record plus both Taken drills, on EVERY stored fight
/// (rows-only: the details tier holds no copy, it exists only on kills
/// where rows already carry the same list). `taken_spells` is the meter's
/// taken-by-ability rows capped at `TAKEN_SPELLS_CAP` by amount with the
/// rest in `other`; `taken_sources` is taken-by-attacker-name under the
/// same cap with its rest in `other_sources` (~5 attackers per player on
/// a boss pull, but a raid night's Σ listed 74 on one player and would
/// have cost 345 KB of rows file — the measurement in
/// `docs/plan-role-pivots-step2b.md`). Both identities hold: Σ kept +
/// rollup = the player's Taken row, on either list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerMitigation {
    pub guid: String,
    pub record: Mitigation,
    pub taken_spells: Vec<Row>,
    pub other: TakenOther,
    pub taken_sources: Vec<Row>,
    pub other_sources: TakenOther,
}

impl PlayerMitigation {
    pub fn to_json(&self) -> Json {
        obj! {
            "guid": Json::str(&*self.guid),
            "record": mitigation_json(&self.record),
            "taken_spells": rows_json(&self.taken_spells),
            "other": self.other.to_json(),
            "taken_sources": rows_json(&self.taken_sources),
            "other_sources": self.other_sources.to_json(),
        }
    }

    /// `None` without a `guid`; a missing record reads as all zeros.
    pub fn from_json(v: &Json) -> Option<Self> {
        Some(Self {
            guid: str_of(v, "guid")?.to_string(),
            record: v
                .get("record")
                .and_then(mitigation_from)
                .unwrap_or_default(),
            taken_spells: rows_from(v.get("taken_spells")),
            other: TakenOther::from_json(v.get("other")),
            taken_sources: rows_from(v.get("taken_sources")),
            other_sources: TakenOther::from_json(v.get("other_sources")),
        })
    }
}

/// R17: the `Mitigation` record as an object — the six amounts by field
/// name, then `misses` as an object keyed by `MissKind::name()`, then
/// (v43) `reduced`. All ten miss kinds are written, zeros included, so the
/// lake's column shape is the same in every file.
pub fn mitigation_json(m: &Mitigation) -> Json {
    let misses = MissKind::ALL
        .iter()
        .map(|k| (k.name().to_string(), Json::num(m.misses_of(*k))))
        .collect();
    obj! {
        "absorbed": Json::u64(m.absorbed),
        "blocked": Json::u64(m.blocked),
        "absorbed_full": Json::u64(m.absorbed_full),
        "blocked_full": Json::u64(m.blocked_full),
        "stagger": Json::u64(m.stagger),
        "stagger_ticked": Json::u64(m.stagger_ticked),
        "misses": Json::Obj(misses),
        // v43 (R17 amendment): armor's share, after the misses so an
        // older record's keys keep their order.
        "reduced": Json::u64(m.reduced),
    }
}

/// `None` unless `v` is an object; every missing key (a miss kind this
/// build knows and the file does not) defaults to 0.
pub fn mitigation_from(v: &Json) -> Option<Mitigation> {
    if !matches!(v, Json::Obj(_)) {
        return None;
    }
    let mut m = Mitigation {
        absorbed: u64_of(v, "absorbed").unwrap_or(0),
        blocked: u64_of(v, "blocked").unwrap_or(0),
        absorbed_full: u64_of(v, "absorbed_full").unwrap_or(0),
        blocked_full: u64_of(v, "blocked_full").unwrap_or(0),
        stagger: u64_of(v, "stagger").unwrap_or(0),
        stagger_ticked: u64_of(v, "stagger_ticked").unwrap_or(0),
        misses: [0; MissKind::COUNT],
        reduced: u64_of(v, "reduced").unwrap_or(0),
    };
    if let Some(misses) = v.get("misses") {
        for kind in MissKind::ALL {
            let n = u32_of(misses, kind.name()).unwrap_or(0);
            if let Some(slot) = m.misses.get_mut(kind.index()) {
                *slot = n;
            }
        }
    }
    Some(m)
}

/// `rows/<id>.json` — the seven views' meter rows (every player, no
/// top-n), the death recaps, (step 2b) every player's mitigation
/// record with both Taken drills. Always written; 12–20 KB for a raid
/// before the mitigation lists, ~45 % more with them.
#[derive(Debug, Clone, PartialEq)]
pub struct FightRows {
    pub schema: u16,
    pub id: String,
    /// Indexed by `View::index()`.
    pub views: [Vec<Row>; View::COUNT],
    pub recaps: Vec<Recap>,
    /// R17: one entry per player with a Taken row; empty on a rows file
    /// written before step 2b (`regrade` fills it).
    pub mitigation: Vec<PlayerMitigation>,
    /// R19 (step 3b): one entry per friendly player with any support given
    /// or received — empty without an Augmentation in the fight, and on a
    /// rows file written before step 3b (`regrade` fills it).
    pub support: Vec<PlayerSupport>,
    /// R18 (step 4b): one entry per friendly player with any uptime cell,
    /// keyed by target; empty on a rows file written before step 4b
    /// (`regrade` fills it).
    pub uptime: Vec<PlayerUptime>,
    /// R18 (step 4b): one entry per friendly player with a nonzero coarse
    /// bucket or any mark — the stored Taken drill's timeline and the
    /// tier-2 Healing drill's; empty on an older rows file.
    pub coarse: Vec<PlayerCoarse>,
    /// R20 (step 5): one entry per friendly player with any shield row;
    /// empty on a rows file written before step 5 (`regrade` fills it).
    pub shields: Vec<PlayerShields>,
    /// R21 (step 6): one entry per friendly player with any stack cell or
    /// debuff seen; empty on a rows file written before step 6 (`regrade`
    /// fills it).
    pub stacks: Vec<PlayerStacks>,
}

impl Default for FightRows {
    fn default() -> Self {
        Self {
            schema: HISTORY_SCHEMA,
            id: String::new(),
            views: Default::default(),
            recaps: Vec::new(),
            mitigation: Vec::new(),
            support: Vec::new(),
            uptime: Vec::new(),
            coarse: Vec::new(),
            shields: Vec::new(),
            stacks: Vec::new(),
        }
    }
}

impl FightRows {
    pub fn rows(&self, view: View) -> &[Row] {
        self.views.get(view.index()).map_or(&[], Vec::as_slice)
    }

    pub fn to_json(&self) -> Json {
        let views = VIEW_KEYS
            .iter()
            .map(|(view, key)| (key.to_string(), rows_json(self.rows(*view))))
            .collect();
        obj! {
            "schema": Json::num(self.schema),
            "id": Json::str(&*self.id),
            "views": Json::Obj(views),
            "recaps": Json::Arr(self.recaps.iter().map(|r| obj! {
                "guid": Json::str(&*r.guid),
                "events": recap_events_json(&r.events),
                "attackers": rows_json(&r.attackers),
                "index": Json::num(r.index),
                "at_ms": Json::num(r.at_ms as f64),
                "dropped": Json::num(r.dropped),
            }).collect()),
            "mitigation": Json::Arr(self.mitigation.iter().map(PlayerMitigation::to_json).collect()),
            "support": Json::Arr(self.support.iter().map(PlayerSupport::to_json).collect()),
            "uptime": Json::Arr(self.uptime.iter().map(PlayerUptime::to_json).collect()),
            "coarse": Json::Arr(self.coarse.iter().map(PlayerCoarse::to_json).collect()),
            "shields": Json::Arr(self.shields.iter().map(PlayerShields::to_json).collect()),
            "stacks": Json::Arr(self.stacks.iter().map(PlayerStacks::to_json).collect()),
        }
    }

    pub fn from_json(v: &Json) -> Option<Self> {
        let (schema, id) = identity(v)?;
        let mut views: [Vec<Row>; View::COUNT] = Default::default();
        if let Some(vs) = v.get("views") {
            for (slot, (_, key)) in views.iter_mut().zip(VIEW_KEYS.iter()) {
                *slot = rows_from(vs.get(key));
            }
        }
        let recaps = v
            .get("recaps")
            .and_then(Json::as_arr)
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        Some(Recap {
                            guid: str_of(r, "guid")?.to_string(),
                            events: rows_from(r.get("events")),
                            attackers: rows_from(r.get("attackers")),
                            index: u32_of(r, "index").unwrap_or(0),
                            at_ms: i64_of(r, "at_ms").unwrap_or(0),
                            dropped: u32_of(r, "dropped").unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mitigation = v
            .get("mitigation")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(PlayerMitigation::from_json).collect())
            .unwrap_or_default();
        let support = v
            .get("support")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(PlayerSupport::from_json).collect())
            .unwrap_or_default();
        let uptime = v
            .get("uptime")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(PlayerUptime::from_json).collect())
            .unwrap_or_default();
        let coarse = v
            .get("coarse")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(PlayerCoarse::from_json).collect())
            .unwrap_or_default();
        let shields = v
            .get("shields")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(PlayerShields::from_json).collect())
            .unwrap_or_default();
        let stacks = v
            .get("stacks")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(PlayerStacks::from_json).collect())
            .unwrap_or_default();
        Some(Self {
            schema,
            id,
            views,
            recaps,
            mitigation,
            support,
            uptime,
            coarse,
            shields,
            stacks,
        })
    }
}

/// One player's detail tier: by-spell and by-target breakdowns for Damage
/// and Healing, and the R12 timelines (1 s buckets + marks). R26 (v36):
/// each by-spell list's ability tree, empty on a file written before it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerDetail {
    pub guid: String,
    pub damage_spells: Vec<Row>,
    pub damage_targets: Vec<Row>,
    pub heal_spells: Vec<Row>,
    pub heal_targets: Vec<Row>,
    pub damage_timeline: Timeline,
    pub heal_timeline: Timeline,
    pub damage_tree: SpellTree,
    pub heal_tree: SpellTree,
    /// v42: the count views' drills — Interrupts, Crowd control and Dispels,
    /// each the player's by-spell and by-target lists — so a stored pull
    /// drills (and compares) them as the live meter does. Empty on a
    /// details file written before v42.
    pub counts: Vec<CountDetail>,
    /// v43 (R27): the player's resources, per power type ascending
    /// (`Segment::energize`). Empty on a details file written before it.
    pub energize: Vec<EnergizeRow>,
    /// v44 (R28): the player's pools second by second, one series per
    /// power type ascending (`Segment::power`) — demoted with the rest of
    /// the details. Empty on a details file written before it.
    pub power: Vec<PowerSeries>,
}

/// v42: the views [`PlayerDetail::counts`] keeps, in order.
pub const COUNT_VIEWS: [View; 3] = [View::Interrupts, View::CrowdControl, View::Dispels];

/// v42: one count view's drill of one player.
#[derive(Debug, Clone, PartialEq)]
pub struct CountDetail {
    pub view: View,
    pub spells: Vec<Row>,
    pub targets: Vec<Row>,
}

impl PlayerDetail {
    /// v42: the player's drill on a count view, when one was kept.
    pub fn count(&self, view: View) -> Option<&CountDetail> {
        self.counts.iter().find(|c| c.view == view)
    }
}

/// `details/<id>.json` — written for kills and for wipes of at least
/// `history_details_min_wipe_secs` (never aborted fights); retention keeps
/// bests and pinned fights and demotes the rest by unlink. 60–120 KB for a
/// raid.
#[derive(Debug, Clone, PartialEq)]
pub struct FightDetails {
    pub schema: u16,
    pub id: String,
    pub players: Vec<PlayerDetail>,
}

impl Default for FightDetails {
    fn default() -> Self {
        Self {
            schema: HISTORY_SCHEMA,
            id: String::new(),
            players: Vec::new(),
        }
    }
}

impl FightDetails {
    pub fn to_json(&self) -> Json {
        obj! {
            "schema": Json::num(self.schema),
            "id": Json::str(&*self.id),
            "players": Json::Arr(self.players.iter().map(|p| obj! {
                "guid": Json::str(&*p.guid),
                "damage_spells": rows_json(&p.damage_spells),
                "damage_targets": rows_json(&p.damage_targets),
                "heal_spells": rows_json(&p.heal_spells),
                "heal_targets": rows_json(&p.heal_targets),
                "damage_timeline": timeline_json(&p.damage_timeline),
                "heal_timeline": timeline_json(&p.heal_timeline),
                "damage_tree": spell_tree_json(&p.damage_tree),
                "heal_tree": spell_tree_json(&p.heal_tree),
                "counts": Json::Arr(p.counts.iter().map(|c| obj! {
                    "view": Json::str(view_key(c.view)),
                    "spells": rows_json(&c.spells),
                    "targets": rows_json(&c.targets),
                }).collect()),
                // v43 (R27): the resources, after the count drills.
                "energize": Json::Arr(p.energize.iter().map(|e| obj! {
                    "power_type": Json::num(e.power_type),
                    "gained": Json::num(e.gained),
                    "wasted": Json::num(e.wasted),
                    "count": Json::num(e.count),
                }).collect()),
                // v44 (R28): the power series, `null` where a second had no
                // report.
                "power": Json::Arr(p.power.iter().map(power_json).collect()),
            }).collect()),
        }
    }

    pub fn from_json(v: &Json) -> Option<Self> {
        let (schema, id) = identity(v)?;
        let players = v
            .get("players")
            .and_then(Json::as_arr)
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        Some(PlayerDetail {
                            guid: str_of(p, "guid")?.to_string(),
                            damage_spells: rows_from(p.get("damage_spells")),
                            damage_targets: rows_from(p.get("damage_targets")),
                            heal_spells: rows_from(p.get("heal_spells")),
                            heal_targets: rows_from(p.get("heal_targets")),
                            damage_timeline: timeline_from(p.get("damage_timeline")),
                            heal_timeline: timeline_from(p.get("heal_timeline")),
                            damage_tree: spell_tree_from(p.get("damage_tree")),
                            heal_tree: spell_tree_from(p.get("heal_tree")),
                            // v42: absent before, and empty then.
                            counts: p
                                .get("counts")
                                .and_then(Json::as_arr)
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|c| {
                                            Some(CountDetail {
                                                view: view_named(str_of(c, "view")?)?,
                                                spells: rows_from(c.get("spells")),
                                                targets: rows_from(c.get("targets")),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            // v43: absent before, and empty then.
                            energize: p
                                .get("energize")
                                .and_then(Json::as_arr)
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|e| {
                                            Some(EnergizeRow {
                                                power_type: u32_of(e, "power_type")?,
                                                gained: f64_of(e, "gained").unwrap_or(0.0),
                                                wasted: f64_of(e, "wasted").unwrap_or(0.0),
                                                count: u32_of(e, "count").unwrap_or(0),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            // v44: absent before, and empty then.
                            power: p
                                .get("power")
                                .and_then(Json::as_arr)
                                .map(|a| a.iter().filter_map(power_from).collect())
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            schema,
            id,
            players,
        })
    }
}

// ---- side tables ----------------------------------------------------------------

/// `loadouts/<hash>.json` — content-addressed by `loadout_hash`; most pulls
/// in a night share one file per player.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredLoadout {
    pub schema: u16,
    pub hash: u64,
    pub loadout: Loadout,
}

impl StoredLoadout {
    pub fn new(loadout: Loadout) -> Self {
        Self {
            schema: HISTORY_SCHEMA,
            hash: loadout_hash(&loadout),
            loadout,
        }
    }

    pub fn to_json(&self) -> Json {
        let l = &self.loadout;
        obj! {
            "schema": Json::num(self.schema),
            "hash": hex(self.hash),
            "spec_id": opt_num(l.spec_id.map(u64::from)),
            "talents": Json::Arr(l.talents.iter().map(|t| obj! {
                "node": Json::num(t.node_id),
                "entry": Json::num(t.entry_id),
                "rank": Json::num(t.rank),
            }).collect()),
            "gear": Json::Arr(l.gear.iter().map(|g| obj! {
                "item": Json::num(g.item_id),
                "ilvl": Json::num(g.ilvl),
                "enchants": u32s_json(&g.enchants),
                "bonus_ids": u32s_json(&g.bonus_ids),
                "gems": u32s_json(&g.gems),
            }).collect()),
            // v43: the stat scalars in the log's order and the auras at
            // the line, after the gear.
            "stats": u32s_json(&l.stats),
            "auras": Json::Arr(l.auras.iter().map(|a| obj! {
                "caster": Json::str(&*a.caster),
                "spell": Json::num(a.spell_id),
                "stacks": Json::num(a.stacks),
            }).collect()),
        }
    }

    /// `None` without `schema` and `hash`. The stored hash is trusted, not
    /// recomputed: a reader must never disagree with the file's own name.
    pub fn from_json(v: &Json) -> Option<Self> {
        let schema = u32_of(v, "schema").and_then(|s| u16::try_from(s).ok())?;
        let hash = from_hex(v.get("hash"))?;
        let talents = v
            .get("talents")
            .and_then(Json::as_arr)
            .map(|a| {
                a.iter()
                    .map(|t| TalentPick {
                        node_id: u32_of(t, "node").unwrap_or(0),
                        entry_id: u32_of(t, "entry").unwrap_or(0),
                        rank: u32_of(t, "rank").unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let gear = v
            .get("gear")
            .and_then(Json::as_arr)
            .map(|a| {
                a.iter()
                    .map(|g| GearItem {
                        item_id: u32_of(g, "item").unwrap_or(0),
                        ilvl: u32_of(g, "ilvl").unwrap_or(0),
                        enchants: u32s_from(g.get("enchants")),
                        bonus_ids: u32s_from(g.get("bonus_ids")),
                        gems: u32s_from(g.get("gems")),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            schema,
            hash,
            loadout: Loadout {
                spec_id: u32_of(v, "spec_id"),
                talents,
                gear,
                // v43: absent on a file written before them, and empty then.
                stats: u32s_from(v.get("stats")),
                auras: v
                    .get("auras")
                    .and_then(Json::as_arr)
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| {
                                Some(LoadoutAura {
                                    caster: str_of(x, "caster")?.to_string(),
                                    spell_id: u32_of(x, "spell")?,
                                    stacks: u32_of(x, "stacks").unwrap_or(0),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        })
    }
}

/// One line of `annotations/<id>.ndjson` — append-only, reserved for roadmap
/// item 4 (coach grades and notes). Its existence protects a fight from
/// retention from v1 on; no tool writes them yet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Annotation {
    pub ts_utc_ms: i64,
    /// `"grade"`, `"note"`, … — item 4 defines the vocabulary.
    pub kind: String,
    pub author: String,
    pub rubric: Option<String>,
    pub body: String,
    pub tags: Vec<String>,
}

impl Annotation {
    pub fn to_json(&self) -> Json {
        obj! {
            "schema": Json::num(HISTORY_SCHEMA),
            "ts_utc_ms": Json::num(self.ts_utc_ms as f64),
            "kind": Json::str(&*self.kind),
            "author": Json::str(&*self.author),
            "rubric": self.rubric.as_deref().map_or(Json::Null, Json::str),
            "body": Json::str(&*self.body),
            "tags": Json::Arr(self.tags.iter().map(|t| Json::str(&**t)).collect()),
        }
    }

    /// `None` without a `kind`.
    pub fn from_json(v: &Json) -> Option<Self> {
        Some(Self {
            ts_utc_ms: i64_of(v, "ts_utc_ms").unwrap_or(0),
            kind: str_of(v, "kind")?.to_string(),
            author: str_of(v, "author").unwrap_or_default().to_string(),
            rubric: str_of(v, "rubric").map(str::to_string),
            body: str_of(v, "body").unwrap_or_default().to_string(),
            tags: v
                .get("tags")
                .and_then(Json::as_arr)
                .map(|a| {
                    a.iter()
                        .filter_map(Json::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
}

// ---- shared pieces ---------------------------------------------------------------

/// A meter / breakdown row. Every field is written, defaults included, so
/// the lake's column set is the same in every file.
pub fn row_json(r: &Row) -> Json {
    obj! {
        "key": Json::str(&*r.key),
        "label": Json::str(&*r.label),
        "amount": Json::u64(r.amount),
        "extra": Json::u64(r.extra),
        "count": Json::u64(r.count),
        "crits": Json::u64(r.crits),
        "per_sec": Json::num(r.per_sec),
        "pct": Json::num(r.pct),
        "class": r.class.map_or(Json::Null, |c| Json::str(class_name(c))),
        "spec": opt_num(r.spec.map(|s| u64::from(s.id()))),
        "hp": r.hp.map_or(Json::Null, |(c, m)| Json::Arr(vec![Json::u64(c), Json::u64(m)])),
        "gain": Json::Bool(r.gain),
        "spell_id": Json::num(r.spell_id),
        "enemy": Json::Bool(r.enemy),
        "school": Json::num(r.school),
        // v44 (R2): a Healing row's eaten part, 0 on every other row.
        "heal_absorbed": Json::u64(r.heal_absorbed),
    }
}

/// `None` without a `key`.
pub fn row_from(v: &Json) -> Option<Row> {
    Some(Row {
        key: str_of(v, "key")?.to_string(),
        label: str_of(v, "label").unwrap_or_default().to_string(),
        amount: u64_of(v, "amount").unwrap_or(0),
        extra: u64_of(v, "extra").unwrap_or(0),
        count: u64_of(v, "count").unwrap_or(0),
        crits: u64_of(v, "crits").unwrap_or(0),
        per_sec: f64_of(v, "per_sec").unwrap_or(0.0),
        pct: f64_of(v, "pct").unwrap_or(0.0),
        class: str_of(v, "class").and_then(class_from_name),
        spec: u32_of(v, "spec").and_then(Spec::from_id),
        hp: v.get("hp").and_then(|h| {
            let a = h.as_arr()?;
            Some((a.first()?.as_u64()?, a.get(1)?.as_u64()?))
        }),
        gain: bool_of(v, "gain").unwrap_or(false),
        spell_id: u32_of(v, "spell_id").unwrap_or(0),
        enemy: bool_of(v, "enemy").unwrap_or(false),
        school: u32_of(v, "school").unwrap_or(0),
        // v35: whose row it is was the answer's to say, never the file's —
        // the store marks it when it answers.
        mine: false,
        // v35: only a recap event carries one (`recap_events_json`); a row
        // written before v35, or any other row, reads as unknown.
        offset_ms: i64_of(v, "offset_ms"),
        // v43: likewise a recap event's alone; absent = unknown.
        absorb: u64_of(v, "absorb"),
        // v44 (R2): absent on a row written before it, and 0 there.
        heal_absorbed: u64_of(v, "heal_absorbed").unwrap_or(0),
    })
}

/// v35 (R9): a death window's events as the rows tier writes them — each a
/// row ([`row_json`]) plus its `offset_ms`, the time before the death, and
/// (v43) its `absorb`, the shields left on the victim, on
/// EVERY event (null only where the meter had none), so the lake's recap
/// column set is the same in every file written from v35 on. Kept off
/// [`row_json`] itself: only a recap event has a time before a death, and
/// every other row of every rows file would carry a null for it.
fn recap_events_json(rows: &[Row]) -> Json {
    Json::Arr(
        rows.iter()
            .map(|r| {
                let mut o = row_json(r);
                if let Json::Obj(fields) = &mut o {
                    fields.push((
                        "offset_ms".to_string(),
                        r.offset_ms.map_or(Json::Null, |v| Json::num(v as f64)),
                    ));
                    // v43: the shields left on the victim, same report as
                    // `hp` (null where the meter had none).
                    fields.push(("absorb".to_string(), r.absorb.map_or(Json::Null, Json::u64)));
                }
                o
            })
            .collect(),
    )
}

/// One timeline mark as details and (4b) the coarse block write it:
/// `kind` is the CODE (an uptime cell's is the name).
pub fn mark_json(m: &Mark) -> Json {
    obj! {
        "at_ms": Json::num(m.at_ms as f64),
        "kind": Json::num(m.kind.code()),
        "label": Json::str(&*m.label),
        "spell_id": Json::num(m.spell_id),
        "dur_ms": Json::num(m.dur_ms as f64),
        // R18 (v24): the caster's guid, written on EVERY mark (empty for
        // item marks) so the SQL column keeps one shape, like `misses`;
        // a pre-v24 file without the key reads empty.
        "src": Json::str(&*m.src),
        // v41: the span had not closed when the fight was read (a player
        // still dead at its end), written on every mark for the same one
        // shape; a file without the key reads false.
        "open": Json::Bool(m.open),
    }
}

/// `None` on an unknown kind code (the mark is dropped, not the list).
pub fn mark_from(m: &Json) -> Option<Mark> {
    let kind = u32_of(m, "kind")
        .and_then(|k| u8::try_from(k).ok())
        .and_then(MarkKind::from_code)?;
    let src = str_of(m, "src").unwrap_or_default().to_string();
    let label = str_of(m, "label").unwrap_or_default().to_string();
    // A record written before v41 says nothing of `open`. Its death marks
    // closed by the old R23 rule — at damage dealt in their name, or at a
    // member's end in a Σ — so an unrezzed one's end is no sighting: read it
    // as never closed, as the graph did then. A rez (a rezzer, or a self-rez's
    // "Death (Reincarnation)") is a real end, and every other kind reads
    // closed.
    let rezzed = !src.is_empty() || label.starts_with("Death (");
    let open = bool_of(m, "open").unwrap_or(kind == MarkKind::Death && !rezzed);
    Some(Mark {
        at_ms: i64_of(m, "at_ms").unwrap_or(0),
        kind,
        label,
        spell_id: u32_of(m, "spell_id").unwrap_or(0),
        src,
        dur_ms: i64_of(m, "dur_ms").unwrap_or(0),
        open,
    })
}

/// A missing or malformed mark list reads as empty.
fn marks_from(v: Option<&Json>) -> Vec<Mark> {
    v.and_then(Json::as_arr)
        .map(|a| a.iter().filter_map(mark_from).collect())
        .unwrap_or_default()
}

pub fn timeline_json(t: &Timeline) -> Json {
    obj! {
        "bucket_ms": Json::num(t.bucket_ms),
        "buckets": Json::Arr(t.buckets.iter().map(|b| Json::u64(*b)).collect()),
        "marks": Json::Arr(t.marks.iter().map(mark_json).collect()),
    }
}

/// R26: a player's ability tree — `groups` and `rows`, each group's `kind`
/// by name, each part's `periodic` a bool.
/// R28 (v44): one power series as the details tier keeps it — its type,
/// largest max, grid and a value or `null` per second.
pub fn power_json(s: &PowerSeries) -> Json {
    obj! {
        "power_type": Json::num(s.power_type),
        "max": Json::num(s.max),
        "bucket_ms": Json::num(s.bucket_ms),
        "per_sec": Json::Arr(s.per_sec.iter().map(|v| v.map_or(Json::Null, Json::num)).collect()),
    }
}

/// R28: `None` without a `power_type`; a value that is not a number reads
/// as no report.
pub fn power_from(v: &Json) -> Option<PowerSeries> {
    Some(PowerSeries {
        power_type: u32_of(v, "power_type")?,
        max: u32_of(v, "max").unwrap_or(0),
        bucket_ms: u32_of(v, "bucket_ms").unwrap_or(1000),
        per_sec: v
            .get("per_sec")
            .and_then(Json::as_arr)
            .map(|a| {
                a.iter()
                    .map(|x| x.as_u64().and_then(|n| u32::try_from(n).ok()))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn spell_tree_json(t: &SpellTree) -> Json {
    obj! {
        "groups": Json::Arr(t.groups.iter().map(|g| obj! {
            "key": Json::str(&*g.key),
            "label": Json::str(&*g.label),
            "spell_id": Json::num(g.spell_id),
            "kind": Json::str(g.kind.name()),
        }).collect()),
        "rows": Json::Arr(t.rows.iter().map(|m| {
            let mut o = obj! {
                "key": Json::str(&*m.key),
                "group": Json::str(&*m.group),
                "casts": Json::u64(m.casts),
                "misses": Json::u64(m.misses),
                "uptime_ms": Json::u64(m.uptime_ms),
                // v43: the casts that began; absent on an older file reads 0.
                "starts": Json::u64(m.starts),
                "parts": Json::Arr(m.parts.iter().map(|p| obj! {
                    "spell_id": Json::num(p.spell_id),
                    "periodic": Json::Bool(p.periodic),
                    "amount": Json::u64(p.amount),
                    "extra": Json::u64(p.extra),
                    "count": Json::u64(p.count),
                    "crits": Json::u64(p.crits),
                }).collect()),
            };
            // v44 (R26): an empowered spell's releases by stage and its
            // cancels — on its row alone; absent elsewhere reads empty.
            if let (Json::Obj(fields), false) = (&mut o, m.empower.is_empty()) {
                fields.push((
                    "empower".to_string(),
                    obj! {
                        "stages": Json::Arr(m.empower.stages.iter().map(|n| Json::u64(*n)).collect()),
                        "cancelled": Json::u64(m.empower.cancelled),
                    },
                ));
            }
            o
        }).collect()),
    }
}

/// R26 (v44): a stored row's empower counts — four stages and the
/// cancels; a missing or short list reads its stages as 0.
fn empower_from(v: &Json) -> Empower {
    let mut e = Empower {
        cancelled: u64_of(v, "cancelled").unwrap_or(0),
        ..Empower::default()
    };
    if let Some(a) = v.get("stages").and_then(Json::as_arr) {
        for (slot, n) in e.stages.iter_mut().zip(a) {
            *slot = n.as_u64().unwrap_or(0);
        }
    }
    e
}

/// R26: a missing tree (a details file written before v36) reads as the
/// empty one — a stored pull's abilities then stand flat, as they did; a
/// malformed entry is dropped, and the lists are re-sorted by key so the
/// lookups hold whatever order the file kept.
pub fn spell_tree_from(v: Option<&Json>) -> SpellTree {
    let Some(v) = v else {
        return SpellTree::default();
    };
    let arr = |key: &str| v.get(key).and_then(Json::as_arr).unwrap_or_default();
    let mut groups: Vec<SpellGroup> = arr("groups")
        .iter()
        .filter_map(|g| {
            Some(SpellGroup {
                key: str_of(g, "key")?.to_string(),
                label: str_of(g, "label")?.to_string(),
                spell_id: u32_of(g, "spell_id").unwrap_or(0),
                kind: GroupKind::from_name(str_of(g, "kind")?)?,
            })
        })
        .collect();
    let mut rows: Vec<SpellMeta> = arr("rows")
        .iter()
        .filter_map(|m| {
            Some(SpellMeta {
                key: str_of(m, "key")?.to_string(),
                group: str_of(m, "group").unwrap_or_default().to_string(),
                casts: u64_of(m, "casts").unwrap_or(0),
                misses: u64_of(m, "misses").unwrap_or(0),
                uptime_ms: u64_of(m, "uptime_ms").unwrap_or(0),
                starts: u64_of(m, "starts").unwrap_or(0),
                // v44: absent on every row but an empowered spell's.
                empower: m.get("empower").map(empower_from).unwrap_or_default(),
                parts: m
                    .get("parts")
                    .and_then(Json::as_arr)
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| {
                                Some(SpellPart {
                                    spell_id: u32_of(p, "spell_id")?,
                                    periodic: bool_of(p, "periodic").unwrap_or(false),
                                    amount: u64_of(p, "amount")?,
                                    extra: u64_of(p, "extra").unwrap_or(0),
                                    count: u64_of(p, "count").unwrap_or(0),
                                    crits: u64_of(p, "crits").unwrap_or(0),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    groups.sort_by(|a, b| a.key.cmp(&b.key));
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    SpellTree { groups, rows }
}

/// A missing or malformed timeline reads as empty, never as an error.
pub fn timeline_from(v: Option<&Json>) -> Timeline {
    let Some(v) = v else {
        return Timeline::default();
    };
    Timeline {
        bucket_ms: u32_of(v, "bucket_ms").unwrap_or(0),
        buckets: v
            .get("buckets")
            .and_then(Json::as_arr)
            .map(|a| a.iter().filter_map(Json::as_u64).collect())
            .unwrap_or_default(),
        marks: marks_from(v.get("marks")),
    }
}

pub fn class_name(c: Class) -> &'static str {
    match c {
        Class::Warrior => "Warrior",
        Class::Paladin => "Paladin",
        Class::Hunter => "Hunter",
        Class::Rogue => "Rogue",
        Class::Priest => "Priest",
        Class::DeathKnight => "DeathKnight",
        Class::Shaman => "Shaman",
        Class::Mage => "Mage",
        Class::Warlock => "Warlock",
        Class::Monk => "Monk",
        Class::Druid => "Druid",
        Class::DemonHunter => "DemonHunter",
        Class::Evoker => "Evoker",
    }
}

pub fn class_from_name(s: &str) -> Option<Class> {
    Some(match s {
        "Warrior" => Class::Warrior,
        "Paladin" => Class::Paladin,
        "Hunter" => Class::Hunter,
        "Rogue" => Class::Rogue,
        "Priest" => Class::Priest,
        "DeathKnight" => Class::DeathKnight,
        "Shaman" => Class::Shaman,
        "Mage" => Class::Mage,
        "Warlock" => Class::Warlock,
        "Monk" => Class::Monk,
        "Druid" => Class::Druid,
        "DemonHunter" => Class::DemonHunter,
        "Evoker" => Class::Evoker,
        _ => return None,
    })
}

fn encounter_json(e: Encounter) -> Json {
    obj! {
        "id": Json::num(e.id),
        "difficulty": Json::num(e.difficulty),
        "group_size": Json::num(e.group_size),
    }
}

fn encounter_from(v: &Json) -> Option<Encounter> {
    Some(Encounter {
        id: u32_of(v, "id")?,
        difficulty: u32_of(v, "difficulty").unwrap_or(0),
        group_size: u32_of(v, "group_size").unwrap_or(0),
    })
}

fn pars_json(p: Option<(i64, i64, i64)>) -> Json {
    p.map_or(Json::Null, |(a, b, c)| {
        Json::Arr(vec![
            Json::num(a as f64),
            Json::num(b as f64),
            Json::num(c as f64),
        ])
    })
}

fn pars_from(v: Option<&Json>) -> Option<(i64, i64, i64)> {
    let a = v?.as_arr()?;
    Some((
        a.first()?.as_i64()?,
        a.get(1)?.as_i64()?,
        a.get(2)?.as_i64()?,
    ))
}

fn parse_build(s: &str) -> (u16, u16, u16) {
    let mut it = s.split('.').map(|p| p.parse::<u16>().unwrap_or(0));
    let mut next = || it.next().unwrap_or(0);
    (next(), next(), next())
}

fn rows_json(rows: &[Row]) -> Json {
    Json::Arr(rows.iter().map(row_json).collect())
}

fn rows_from(v: Option<&Json>) -> Vec<Row> {
    v.and_then(Json::as_arr)
        .map(|a| a.iter().filter_map(row_from).collect())
        .unwrap_or_default()
}

fn u32s_json(v: &[u32]) -> Json {
    Json::Arr(v.iter().map(|n| Json::num(*n)).collect())
}

fn u32s_from(v: Option<&Json>) -> Vec<u32> {
    v.and_then(Json::as_arr)
        .map(|a| {
            a.iter()
                .filter_map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok()))
                .collect()
        })
        .unwrap_or_default()
}

// ---- affiliations (the wowdps addon) ----------------------------------------

/// The SavedVariables global the wowdps addon writes (`addon/wowdps.lua`).
pub const ADDON_GLOBAL: &str = "WOWDPS_DATA";

/// `affiliations/<guid>.json` — one player's guild as the wowdps addon last
/// saw them (spec §9a). The daemon writes these from the addon's
/// SavedVariables and joins `guild` onto a card's players at READ time;
/// no card ever stores one, because the addon's file lands after the
/// night it describes (the game flushes SavedVariables on logout).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Affiliation {
    pub schema: u16,
    /// The unit guid the combat log uses — the join key.
    pub guid: String,
    pub name: String,
    /// The normalized realm name (`"Area52"`, as the log spells it).
    pub realm: String,
    /// `""` is a player SEEN without a guild; the addon writes no record
    /// at all for a unit it could not tell about.
    pub guild: String,
    /// The guild's realm when it is not the player's own.
    pub guild_realm: Option<String>,
    pub rank: Option<String>,
    /// The class file name (`"WARRIOR"`), as the addon read it.
    pub class: Option<String>,
    pub faction: Option<String>,
    /// One of the account's own characters — the logger, whichever alt.
    pub mine: bool,
    pub seen_utc_ms: i64,
    /// The `WTF/Account/<name>` the record came from.
    pub account: String,
}

impl Affiliation {
    pub fn to_json(&self) -> Json {
        obj! {
            "schema": Json::num(self.schema),
            "guid": Json::str(&*self.guid),
            "name": Json::str(&*self.name),
            "realm": Json::str(&*self.realm),
            "guild": Json::str(&*self.guild),
            "guild_realm": self.guild_realm.as_deref().map_or(Json::Null, Json::str),
            "rank": self.rank.as_deref().map_or(Json::Null, Json::str),
            "class": self.class.as_deref().map_or(Json::Null, Json::str),
            "faction": self.faction.as_deref().map_or(Json::Null, Json::str),
            "mine": Json::Bool(self.mine),
            "seen_utc_ms": Json::num(self.seen_utc_ms as f64),
            "account": Json::str(&*self.account),
        }
    }

    /// `None` without `schema` and a `guid`.
    pub fn from_json(v: &Json) -> Option<Self> {
        let schema = u32_of(v, "schema").and_then(|s| u16::try_from(s).ok())?;
        let guid = str_of(v, "guid").filter(|g| !g.is_empty())?.to_string();
        Some(Self {
            schema,
            guid,
            name: str_of(v, "name").unwrap_or_default().to_string(),
            realm: str_of(v, "realm").unwrap_or_default().to_string(),
            guild: str_of(v, "guild").unwrap_or_default().to_string(),
            guild_realm: str_of(v, "guild_realm").map(str::to_string),
            rank: str_of(v, "rank").map(str::to_string),
            class: str_of(v, "class").map(str::to_string),
            faction: str_of(v, "faction").map(str::to_string),
            mine: bool_of(v, "mine").unwrap_or(false),
            seen_utc_ms: i64_of(v, "seen_utc_ms").unwrap_or(0),
            account: str_of(v, "account").unwrap_or_default().to_string(),
        })
    }

    /// One `players[guid]` record of the addon's table. `None` for a
    /// record without a name (a truncated or foreign entry).
    fn from_lua(guid: &str, rec: &Lua, mine: bool, account: &str) -> Option<Self> {
        let text = |key: &str| rec.get(key).and_then(Lua::as_str).map(str::to_string);
        let name = text("name").filter(|n| !n.is_empty())?;
        Some(Self {
            schema: HISTORY_SCHEMA,
            guid: guid.to_string(),
            name,
            realm: text("realm").unwrap_or_default(),
            guild: text("guild").unwrap_or_default(),
            guild_realm: text("guild_realm").filter(|r| !r.is_empty()),
            rank: text("rank"),
            class: text("class"),
            faction: text("faction"),
            mine,
            // The addon writes `GetServerTime()`, whole seconds UTC.
            seen_utc_ms: rec
                .get("seen")
                .and_then(Lua::as_f64)
                .map_or(0, |s| (s * 1000.0) as i64),
            account: account.to_string(),
        })
    }

    /// Every record in one account's `wowdps.lua` (its `WOWDPS_DATA`
    /// global): `players` keyed by guid, `characters` marking the
    /// account's own. A file without the global is an empty answer, not
    /// an error — the addon may be installed and never have run.
    pub fn read_saved_variables(text: &str, account: &str) -> Result<Vec<Self>, String> {
        let globals = crate::lua::parse(text).map_err(|e| e.to_string())?;
        let Some((_, data)) = globals.iter().find(|(n, _)| n == ADDON_GLOBAL) else {
            return Ok(Vec::new());
        };
        let mine: Vec<&str> = data
            .get("characters")
            .and_then(Lua::as_table)
            .map(|t| {
                t.iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .filter_map(|(k, _)| k.as_str())
                    .collect()
            })
            .unwrap_or_default();
        Ok(data
            .get("players")
            .and_then(Lua::as_table)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| {
                        let guid = k.as_str()?;
                        Self::from_lua(guid, v, mine.contains(&guid), account)
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn opt_num(n: Option<u64>) -> Json {
    n.map_or(Json::Null, Json::u64)
}

fn opt_bool(b: Option<bool>) -> Json {
    b.map_or(Json::Null, Json::Bool)
}

/// `(schema, id)` — the two fields no document reads without.
fn identity(v: &Json) -> Option<(u16, String)> {
    let schema = u32_of(v, "schema").and_then(|s| u16::try_from(s).ok())?;
    let id = str_of(v, "id")?;
    (!id.is_empty()).then(|| (schema, id.to_string()))
}

fn str_of<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

fn u64_of(v: &Json, key: &str) -> Option<u64> {
    v.get(key)?.as_u64()
}

fn u32_of(v: &Json, key: &str) -> Option<u32> {
    u64_of(v, key).and_then(|n| u32::try_from(n).ok())
}

fn i64_of(v: &Json, key: &str) -> Option<i64> {
    v.get(key)?.as_i64()
}

fn f64_of(v: &Json, key: &str) -> Option<f64> {
    v.get(key)?.as_f64()
}

fn bool_of(v: &Json, key: &str) -> Option<bool> {
    v.get(key)?.as_bool()
}
