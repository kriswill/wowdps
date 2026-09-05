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
use crate::obj;
use wowdps_model::{
    Class, Encounter, GearItem, Loadout, Mark, MarkKind, MissKind, Mitigation, Role, Row, Spec,
    TalentPick, Timeline, View,
};

/// Version of every document's shape. Independent of `PROTO_VERSION`: the
/// socket can move without the files moving. A record whose `schema` is
/// older than this is rewritten by the daemon on its next visit; a breaking
/// change is a new directory (`v2/`) plus a migrator, never in-place edits.
pub const HISTORY_SCHEMA: u16 = 1;

/// The seven views (R17's Taken last) in the order their rows are stored,
/// each with the key its rows sit under in a rows document.
pub const VIEW_KEYS: [(View, &str); View::COUNT] = [
    (View::Damage, "damage"),
    (View::Healing, "healing"),
    (View::Interrupts, "interrupts"),
    (View::CrowdControl, "cc"),
    (View::Dispels, "dispels"),
    (View::Deaths, "deaths"),
    (View::Taken, "taken"),
];

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
    /// `Mitigation::prevented` — full absorbs + full blocks, the amounts a
    /// miss carried that never became Taken. 0 on an older card.
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
    /// Reserved for ruling R16 (min observed boss health); never written yet.
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
                self.players.iter().map(|p| p.to_json_in(Some(self.duration_ms))).collect()
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

    /// R17: `mitigated / (taken + prevented)` × 100 through the model's
    /// one [`wowdps_model::mitigated_pct`]. Derived the way `role` is:
    /// never a struct field, written to JSON as `mitigated_pct` for readers
    /// that cannot do the arithmetic themselves (DuckDB), ignored on read.
    /// 0.0 on a card without the tank measures.
    pub fn mitigated_pct(&self) -> f64 {
        wowdps_model::mitigated_pct(self.mitigated, self.taken, self.prevented)
    }

    /// R19 (step 3b): the player's effective damage — `damage` minus the
    /// shares supporters gave them plus the shares they gave others —
    /// through the model's one [`wowdps_model::effective`] (clamped at 0,
    /// never a wrap). Equal to `damage` on a card without support scalars,
    /// so an older card's effective is its raw damage.
    pub fn effective(&self) -> u64 {
        wowdps_model::effective(self.damage, self.support_received, self.support_given)
    }

    /// Effective damage per second over the card's `duration_ms` — the
    /// SAME arithmetic `Meter::finish_rows` uses for a rate row's
    /// `per_sec` (`amount as f64 / secs` with `secs = duration_ms as f64
    /// / 1000.0`), so on a fight without support it is `dps` bit for bit,
    /// which is what lets grading and trend rank it with no predicate.
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

    /// The player's line inside a card of `duration_ms`; `effective_dps`
    /// is derived from it (`None` writes `null`).
    pub fn to_json_in(&self, duration_ms: Option<i64>) -> Json {
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
            "overheal": Json::u64(self.overheal),
            "absorbed": Json::u64(self.absorbed),
            "support_given": Json::u64(self.support_given),
            "support_received": Json::u64(self.support_received),
            "healed_received": Json::u64(self.healed_received),
            "self_healed": Json::u64(self.self_healed),
            "effective_dps": duration_ms.map_or(Json::Null, |d| Json::num(self.effective_dps(d))),
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
        })
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
/// name, then `misses` as an object keyed by `MissKind::name()`. All ten
/// miss kinds are written, zeros included, so the lake's column shape is
/// the same in every file.
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
                "events": rows_json(&r.events),
                "attackers": rows_json(&r.attackers),
            }).collect()),
            "mitigation": Json::Arr(self.mitigation.iter().map(PlayerMitigation::to_json).collect()),
            "support": Json::Arr(self.support.iter().map(PlayerSupport::to_json).collect()),
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
        Some(Self {
            schema,
            id,
            views,
            recaps,
            mitigation,
            support,
        })
    }
}

/// One player's detail tier: by-spell and by-target breakdowns for Damage
/// and Healing, and the R12 timelines (1 s buckets + marks).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlayerDetail {
    pub guid: String,
    pub damage_spells: Vec<Row>,
    pub damage_targets: Vec<Row>,
    pub heal_spells: Vec<Row>,
    pub heal_targets: Vec<Row>,
    pub damage_timeline: Timeline,
    pub heal_timeline: Timeline,
}

/// `details/<id>.json` — kills, bests and pinned fights only; demoted by
/// unlink under retention. 60–120 KB for a raid.
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
    })
}

pub fn timeline_json(t: &Timeline) -> Json {
    obj! {
        "bucket_ms": Json::num(t.bucket_ms),
        "buckets": Json::Arr(t.buckets.iter().map(|b| Json::u64(*b)).collect()),
        "marks": Json::Arr(t.marks.iter().map(|m| obj! {
            "at_ms": Json::num(m.at_ms as f64),
            "kind": Json::num(m.kind.code()),
            "label": Json::str(&*m.label),
            "spell_id": Json::num(m.spell_id),
            "dur_ms": Json::num(m.dur_ms as f64),
        }).collect()),
    }
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
        marks: v
            .get("marks")
            .and_then(Json::as_arr)
            .map(|a| {
                a.iter()
                    .filter_map(|m| {
                        Some(Mark {
                            at_ms: i64_of(m, "at_ms").unwrap_or(0),
                            kind: u32_of(m, "kind")
                                .and_then(|k| u8::try_from(k).ok())
                                .and_then(MarkKind::from_code)?,
                            label: str_of(m, "label").unwrap_or_default().to_string(),
                            spell_id: u32_of(m, "spell_id").unwrap_or(0),
                            src: str_of(m, "src").unwrap_or_default().to_string(),
                            dur_ms: i64_of(m, "dur_ms").unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
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
