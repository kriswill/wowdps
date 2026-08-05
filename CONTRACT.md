# Module contract (coordinator-owned; changes require coordinator sign-off)

Workspace of five crates around a client/server split:

- `wowdps-model` — domain types only (`View`, `Row`, `Class`, `Spec`, `SegmentKind`,
  `SegmentId`, `SegmentInfo`, `ListRow`, `Screen`, `Pane`, `Drill`, `Action`, `fmt`).
  Zero dependencies, no I/O, no parser.
- `wowdps-core` — the engine: `parser`, `meter`, `index`, `tail`. Re-exports model.
  Only the daemon runs it.
- `wowdps-proto` — wire codec (`wire`, `msg`), client library (`client`) and the
  client-side state machine (`state::ClientState`). Depends on model only — a crate
  linking proto cannot parse a combat log even by accident.
- `wowdps-daemon` — the headless daemon: one tail/index/meter pipeline serving every
  client over a unix socket, plus the game watcher and overlay supervisor.
- Binaries: `wowdps` (daemon + launcher + TUI client; links core transitively, but
  `crates/tui/src` never names engine modules — gated by `tests/no_engine.rs`) and
  `wowdps-gui` (window + `--overlay`; pure client, deps model + proto only).

Field lists may grow; names/shapes below are the agreed interface.

## src/parser.rs (owner: core)

```rust
pub struct LogLine {
    pub ts_ms: i64,                       // unix-ish ms, monotonic within a file
    pub event: Event,
    pub owner_hint: Option<OwnerHint>,    // advanced-block pet ownership; additive, ignorable
    pub hp_hint: Option<HpHint>,          // advanced-block (current, max) HP of the unit it
}                                         // describes — carried even on Event::Other lines (R9)

pub struct Unit { pub guid: String, pub name: String, pub flags: u32 }
impl Unit { pub fn is_player(&self) -> bool; pub fn is_pet_or_guardian(&self) -> bool; }

pub enum Event {
    Version { log_version: u32, advanced: bool },
    EncounterStart { id: u32, name: String, difficulty: u32, group_size: u32 },
    EncounterEnd   { id: u32, name: String, success: bool },
    CombatantInfo  { guid: String },
    Damage { src: Unit, dst: Unit, spell: Option<Spell>, amount: u64, overkill: i64, absorbed: u64, critical: bool, periodic: bool },
    Heal   { src: Unit, dst: Unit, spell: Spell, amount: u64, overheal: u64, absorbed: u64, critical: bool },
    Absorbed { src: Unit, dst: Unit, absorber: Unit, spell: Option<Spell>, absorb_spell: Spell, amount: u64 },
    Interrupt { src: Unit, dst: Unit, spell: Spell, interrupted_spell: Spell },
    AuraApplied { src: Unit, dst: Unit, spell: Spell, aura_type: AuraType }, // Buff | Debuff
    Dispel { src: Unit, dst: Unit, spell: Spell, dispelled_spell: Spell },
    Summon { owner: Unit, pet: Unit },
    Death  { unit: Unit },
    ZoneChange         { map_id: u32, name: String, difficulty: u32 },  // R10; difficulty 0 = open world
    ChallengeModeStart { map_id: u32, key_level: u32 },                 // R10
    ChallengeModeEnd   { map_id: u32, success: bool },                  // R10
    Other,                       // recognized-as-log-line but not modeled; never an error
}

pub struct Spell { pub id: u32, pub name: String, pub school: u32 }
pub enum AuraType { Buff, Debuff }

/// None for blank/malformed lines. Unknown events => Some(LogLine{event: Other, ..}).
pub fn parse_line(line: &str) -> Option<LogLine>;
```

## src/meter.rs (owner: core)

```rust
pub struct Meter { /* feeds lines, owns segments + visits */ }
impl Meter {
    pub fn new() -> Self;
    pub fn feed(&mut self, line: LogLine);
    pub fn segments(&self) -> &[Segment];          // history, oldest first; last = live/current
    pub fn visits(&self) -> &[Visit];              // R10: instance visits, file order (= ordinals)
    pub fn overall(&self, ordinal: u32) -> Option<Segment>; // R10: members merged; None until one exists
    pub fn current_index(&self) -> usize;
}

pub enum SegmentKind { Encounter, Trash, Overall }  // Overall never appears in segments() (R10)
pub struct Segment {
    pub kind: SegmentKind,
    pub name: String,              // encounter name, "Trash", or the visit's display name
    pub start_ms: i64,
    pub end_ms: Option<i64>,       // None while live
    pub success: Option<bool>,
    pub visit: Option<u32>,        // R10: ordinal of the visit this was recorded in
}
pub struct Visit {                 // R10: one contiguous stay in instanced content
    pub map_id: u32, pub difficulty: u32, pub name: String,
    pub key_level: Option<u32>, pub keyed: bool,
    pub start_ms: i64, pub end_ms: Option<i64>,   // None while in progress (incl. suspended)
    pub completed: Option<bool>,   // keystone runs: CHALLENGE_MODE_END's success flag
}
impl Visit { pub fn display_name(&self) -> String; }  // "Skyreach +10" for keys, else the zone name
impl Segment {
    pub fn duration_ms(&self, now_ms: i64) -> i64;
    pub fn last_combat_ms(&self) -> i64;           // R10: the Overall merge's deterministic "now"
    pub fn absorb(&mut self, other: &Segment);     // R10: merge counters (Overall aggregation)
    /// Rows for a view, sorted desc by amount (Deaths: first-death order, R9).
    /// pct is of view total.
    pub fn rows(&self, view: View) -> Vec<Row>;
    /// Drilldown for one player: (by-spell rows, by-target rows) for the view.
    /// Deaths: (recap timeline newest-first, attacker totals) instead (R9).
    pub fn breakdown(&self, player_guid: &str, view: View) -> (Vec<Row>, Vec<Row>);
}

pub enum View { Damage, Healing, Interrupts, CrowdControl, Dispels, Deaths }
pub struct Row {
    pub key: String,               // player guid (meter) / spell or target name (breakdown)
    pub label: String,             // display name
    pub amount: u64,               // damage done, healing done, or event count
    pub extra: u64,                // overheal for Healing; overkill for Damage; else 0
    pub count: u64,                // contributing events (hits/ticks/heals; absorbs count but never crit)
    pub crits: u64,                // how many of `count` were critical; crit_pct() = crits/count
    pub per_sec: f64,              // DPS/HPS; 0.0 for count views
    pub pct: f64,                  // 0..100 of view total
    pub class: Option<Class>,      // COMBATANT_INFO specID, else R8 inference; bars render in class color
    pub spec: Option<Spec>,        // COMBATANT_INFO specID, else R8 inference from spec-unique casts
    pub hp: Option<(u64, u64)>,    // death-recap rows only (R9): victim (current, max) HP post-event
    pub gain: bool,                // death-recap rows only (R9): heal / consumed absorb, not damage
}
```

Semantics (RULINGS R1-R10, binding for meter AND fixture expected values):
- R1 Damage rows: amount = per-event `amount + absorbed-field` (absorbed-by-shield damage
  counts as damage done, meter convention); extra = overkill clamped to >=0. Count
  SWING_DAMAGE only (SWING_DAMAGE_LANDED -> Other); `*_SUPPORT` -> Other; DAMAGE_SPLIT
  excluded from offensive totals.
- R2 Healing rows: amount = effective healing (amount - overheal); extra = overheal.
  SPELL_ABSORBED credits the ABSORBER with healing (no overheal component). The
  damage-event absorbed field never contributes to any healing number. Stagger/
  cheat-death self-absorbs (114556, 31850, 31230, 115069) excluded from healing.
- R3 SPELL_ABSORBED is the sole source for absorb-as-healing; the damage-event absorbed
  field is the sole source for absorb-as-damage. Different views, different actors — no
  double count.
- R4 Segments close exactly at ENCOUNTER_END (known ~1-3% DoT-tail divergence vs
  Warcraft Logs; accepted, no grace window). R10 amendment: every ZONE_CHANGE also
  closes the open Trash segment — a teleport is a hard location break, and without
  it pre-instance trash would bleed into (and out of) a visit.
- R7 Duration semantics: Encounter segments = ENCOUNTER_START..ENCOUNTER_END exactly.
  Trash segments = FIRST..LAST combat event inside the segment (active combat time,
  like in-game meters) — never open..close, which counts idle time and deflates DPS.
- R5 Pet by-spell breakdown row label: "{spell} ({petName})".
- R6 Mid-log COMBAT_LOG_VERSION = hard boundary: close open segment, reset pet-owner
  map, and close the open visit (R10).
- R8 Class/spec inference: outside instances COMBATANT_INFO never fires, so a player's
  class (and, when the spell is unique to one specialization, spec) is inferred from
  player-sourced spell events — Damage/Heal/Interrupt/Dispel/AuraApplied via `src`,
  SPELL_ABSORBED via the absorbing shield's caster — against the generated table
  `core/src/class_spells.rs` (spell id → class/spec; built by `tools/gen-class-spells.py`
  from wago.tools DB2 exports: class skill lines + SpecializationSpells + trait trees;
  spells castable by more than one class are excluded, spells granted class-wide carry
  no spec). Inference is SEGMENT-LOCAL: it writes only the open segment, never the
  carried-forward maps, so lazy loading (which replays only seed lines + the slice)
  reproduces it exactly. COMBATANT_INFO is authoritative — it overwrites inference and
  is the only class/spec source that persists across segments. Inference never opens or
  extends a segment (scanner lockstep).
- R9 Deaths & the death recap. `rows(Deaths)` lists players in FIRST-death order (not
  by count, not alphabetical). Each Segment keeps a bounded per-player ring
  (`RECAP_CAP = 32`) of recent events on that player: damage hits (amount = the
  per-event `amount` alone — the absorbed part never touched their health and appears
  as its own gain entry) and gains (heals at effective value with overheal in extra;
  consumed absorbs via SPELL_ABSORBED). UNIT_DIED drains the ring into that player's
  recap — latest death wins — so `breakdown(guid, Deaths)` returns (timeline
  newest-first, so the killing blow with its overkill-in-extra leads; attacker totals
  sorted desc, gains excluded, source-less damage bucketed under its spell name).
  Timeline rows carry `hp`/`gain`; labels are "{spell} ({source})", spell alone when
  the source unit is nil. HP comes from the line's own advanced block when it
  describes the victim, else it back-fills onto the newest HP-less entry from the
  next advanced line describing them within 1s (SWING_DAMAGE -> its LANDED twin;
  SPELL_ABSORBED -> the paired damage line). Health reports and recap bookkeeping
  never open or extend a segment (scanner lockstep), and the ring is segment-local,
  so lazy loading reproduces recaps exactly.
- R10 Instance visits & the per-visit Overall. A ZONE_CHANGE with difficulty != 0
  opens a *visit* (map_id + difficulty + zone name); ordinals index the file's visit
  table in order. Zoning out (difficulty 0) SUSPENDS the visit — segments recorded
  outside carry no visit — and re-entering the same (map_id, difficulty) resumes it;
  entering a different instance closes it. CHALLENGE_MODE_START stamps the current
  visit's key level the first time; a second START on the same map is a new key: the
  visit (and any open trash) closes and a fresh one opens. CHALLENGE_MODE_END counts
  only for a keyed visit (the zeroed reset the game fires on entry, before any START,
  is ignored) and sets `completed` from its success flag. Segments opened while zoned
  in carry the visit's ordinal — that ordinal is the instance id associated with all
  counters. The visit's OVERALL (`Meter::overall`) is a synthetic
  `SegmentKind::Overall` segment: every member's counters merged (tallies sum;
  identity maps union, later member wins; death order first-occurrence across
  members; each player's latest recap wins), duration = the SUM of member durations
  (R7 applied per member, an open member cut at its last combat event), success =
  `completed`, name = `Visit::display_name()`. Live and lazy paths both build the
  Overall by merging members, so index-then-lazy equals full replay by construction.
  A scan cut mid-visit splits the visit at `live_offset`: the `open_visit` prefix
  carries only members closed before it — bytes and clock — and the open member
  belongs exclusively to the live tail (which replays it from its first line), so
  merging prefix + live counts every member exactly once.
  ZONE_CHANGE / CHALLENGE_MODE_* lines are SEED lines: replaying them ahead of any
  slice (or the live tail) reconstructs the visit table with file-consistent
  ordinals everywhere. In the combined segment list the Overall row precedes its
  visit's first member, and it exists only once the visit has a member.
- Interrupt/CC drill labels: the Interrupts by-spell pane answers "what got kicked" —
  "{interrupted spell} ({interrupt ability})"; the CrowdControl pane answers "who got
  locked down" — "{cc spell} ({victim})". Meter-row counts are unchanged.
- Pet/guardian attribution: damage/heals by a unit summoned by a player (SPELL_SUMMON
  or advanced-field ownerGUID) count toward the owner; label "Owner (Pet)" appears only
  in breakdown by-spell rows, not as separate meter rows.
- Encounter segmentation: ENCOUNTER_START opens an Encounter segment (closing any open
  one), ENCOUNTER_END closes it. Damage outside encounters accrues to a Trash segment;
  a new Trash segment starts after >60s with no combat events.
- Only players (and their pets) get meter rows. Deaths view: player deaths, amount=1 per death.
- CrowdControl view counts AuraApplied debuffs whose spell is in a small built-in CC
  spell-school/mechanic list (loss-of-control: stuns, roots, incaps, fears — keep a
  `const CC_SPELLS`/heuristic; exactness not gated).

## src/index.rs (owner: core)

Fast structural scan: segment boundaries + byte ranges, no per-event parsing. Startup
shows the whole segment list from this index in <1s on a 300MB+ log; a segment's events
are parsed only when opened (`load_segment` + `Meter::feed`). The scanner mirrors
`Meter::feed`'s segmentation rules (ENCOUNTER_START/END, R6, R7) exactly; parity with a
full replay — same segments, same `rows()`, same breakdowns (death recaps included),
same classes and specs (COMBATANT_INFO-derived and R8-inferred alike) — is gated by
fixture tests in the module.

```rust
pub struct SegmentMeta {
    pub kind: SegmentKind,
    pub name: String,              // encounter name, "Trash", or the visit's display name
    pub start_ms: i64,
    pub end_ms: Option<i64>,       // None only on the trailing open segment / open visit
    pub success: Option<bool>,
    pub duration_ms: i64,          // R7 semantics; Overall = sum of member durations (R10)
    pub byte_range: (u64, u64),    // [start, end) file offsets of the slice
    pub seeds: Vec<(u64, u64)>,    // earlier SPELL_SUMMON/COMBATANT_INFO/VERSION/ZONE/CM lines
    pub visit: Option<u32>,        // R10: member's visit ordinal; on Overall, the visit itself
}
pub struct Index {
    pub segments: Vec<SegmentMeta>,   // closed, oldest first
    pub overalls: Vec<SegmentMeta>,   // R10: closed visits' Overall metas (ranges overlap members)
    pub open_visit: Option<SegmentMeta>, // R10: in-progress visit's prefix — closed members only, range ends at live_offset; present once a member has closed
    pub open: Option<SegmentMeta>,    // trailing in-progress segment, if any
    pub live_offset: u64,             // where the live tail starts emitting lines
    pub scanned: u64,
    pub checkpoint: ScanState,        // resumable state at the last clean boundary
}
/// Scanner state at a clean boundary (no open segment; an open *visit* is
/// carried in `visit`); resuming from it reproduces a full scan exactly.
/// This is what the daemon's index cache persists so a 300MB log costs one
/// full scan per file, ever.
pub struct ScanState {
    pub segments: Vec<SegmentMeta>,
    pub overalls: Vec<SegmentMeta>,   // R10
    pub seeds: Vec<(u64, u64)>,
    pub last_combat_ms: Option<i64>,
    pub visit_count: u32,             // R10: ordinals assigned so far
    pub visit: Option<VisitScan>,     // R10: the visit in progress at `offset`
    pub offset: u64,
}
pub fn scan<R: Read>(reader: &mut R) -> Index;
pub fn scan_from<R: Read>(reader: &mut R, state: ScanState) -> Index; // reader at state.offset
/// Seed lines first, then the slice: feeding these through a fresh Meter
/// reproduces the segment (incl. cross-segment pet ownership and classes).
pub fn load_segment(path: &Path, meta: &SegmentMeta) -> io::Result<Vec<String>>;
```

## src/tail.rs (owner: core; consumed only by the daemon)

`Tailer` yields events for (a) one file or (b) the newest `WoWCombatLog*.txt` in a
directory, following growth and rotating to a newer file when one appears. Polling
(~200ms), no notify dependency. On open/rotate it emits `Switched`, then
`Index { index, file_age_ms }` (one structural scan — injectable via
`Tailer::with_scan`, which is where the daemon's index cache plugs in), then `Lines`
starting at the index's `live_offset` — history is never replayed line by line.
`CaughtUp` fires once when the backlog is drained; `Lines` after it are fresh combat.

## Wire protocol (owner: proto) — `PROTO_VERSION = 5`

Transport: unix socket `$XDG_RUNTIME_DIR/wowdps/wowdps-v<PROTO_VERSION>.sock`
(fallback `/tmp/wowdps-<uid>/`, dir 0700, ownership verified). The version lives in
the socket *name*: version skew is structurally impossible, a new client simply
spawns its own daemon and the old one idle-exits.

Framing: `u32 len (LE) | u8 tag | body`, `len` covers tag+body, `MAX_FRAME` 16 MiB.
Primitives: fixed-width LE integers, f64 as bits, bool as 0/1 byte, string =
u32 len + UTF-8, Option = presence byte, Vec = u32 count + items. Decoding returns
`Result` — truncation, bad tags, bad bools, bad UTF-8 and lying counts are errors,
never panics or attacker-sized allocations.

Messages (tags): ClientMsg `Hello 0x01`, `Watch 0x02`, `GetStatus 0x03`,
`VisibilityChanged 0x04`, `Shutdown 0x05` (accepted pre-handshake, so `--stop`
always works). DaemonMsg `HelloAck 0x81`, `Snapshot 0x82`, `SegmentList 0x83`,
`SegmentOpened 0x84`, `LoadFailed 0x85`, `Status 0x86`, `SetVisible 0x87`,
`Fatal 0x88`. A `Watch` carries a `Cursor` — `List`, or
`Segment { SegmentRef (Live | Id), View, top_n, drill }` — and replaces any prior
cursor; the daemon pushes snapshots for exactly what is watched, breakdown included
when drilled. One standing exception: whenever the segment id table changes shape
(a segment appears, the file rotates), the daemon broadcasts a fresh `SegmentList`
to every session regardless of cursor. Off-list navigation resolves neighbors
through that table, and `SegmentOpened` alone cannot keep it complete: the log
arrives in multi-minute flush bursts, and a segment that opens *and* closes inside
one tail batch never announces itself (`Opened` covers only a batch's still-open
tail).

Guarantees:
- `SegmentId`s are monotonic for the daemon's lifetime and never reused; after log
  rotation a stale id resolves to `LoadFailed(Rotated | NotFound)`, never to another
  file's fight. A changed `source` on any snapshot means rotation: clients reset and
  re-`Watch`.
- Snapshot/list `seq` is per-session monotonic. Snapshots are idempotent: a lagging
  client is caught up by dropping stale ones (the client library coalesces to the
  newest per (segment, view)); control messages are ordered and never dropped.
- Encoded shapes are pinned by golden-byte tests in `crates/proto/tests/codec.rs`;
  changing any shape means bumping `PROTO_VERSION` (which renames the socket) and
  re-blessing them. (v2: `Row` gained a trailing u16 Blizzard specID, 0 = none —
  sent as the raw id so an unknown value decodes to `None`, never an error.
  v3: `Row` gained trailing u64 `count` + u64 `crits`. v5: `SegmentKind` gained
  `Overall` (code 2) and `SegmentInfo`/`ListRow` gained a trailing Option<u32>
  `instance` — the R10 visit ordinal.)

Client state (owner: proto): `state::ClientState` holds screen/view/selection/drill
plus the cached last snapshot; `apply(Action)`/`on_msg(DaemonMsg)` return the
`ClientMsg`s to send. Held-key `Up`/`Down` clamps against the cache and never
round-trips. Keybinds (owner: clients): list — `j/k`/arrows move, `Enter` opens,
`q` quit. Meter — `d/h/i/c/x/K` views (capital K — lowercase k moves), `[`/`]`
cycle segments, `Enter` drilldown, `Esc` back (drilldown, then list), `q` quit.

## Daemon (owner: daemon)

One process owns bytes → rows: tail thread, engine (live meter + index + stable ids
+ LRU of ≤16 lazily parsed segments), loader worker pool (historical parses never run
on the hub thread), hub (session table, 10 Hz changed-only pushes), game watcher
(3s /proc sweep for a case-insensitive `game_process` substring), overlay supervisor
(spawns/hides/terminates `wowdps-gui --overlay` on game transitions; a manual hide
sticks until the next transition; spawn failures surface in `Status`). Single
instance via a lockfile taken *before* the stale socket is unlinked. Idle-exit when
the last watching session (or overlay child / exit grace) is gone, unless `--linger`.
Config `~/.config/wowdps/config.toml`, read at startup with a section-aware
toml-subset reader: `logs_dir`, `game_process`, `auto_overlay`,
`overlay_exit_grace_secs` (gui keys belong to the gui, which still writes the file
with the real `toml` crate). The only persistence is the index-checkpoint cache in
`$XDG_CACHE_HOME/wowdps/index` — never parsed meters, which is how a cache would
become an event store by accident.

CLI: `wowdps [--file|--logs]` (TUI client; source conflict with a running daemon is
a hard error naming both), `wowdps --gui`, `wowdps --daemon [--linger] [--file|--logs]`,
`wowdps --status`, `wowdps --stop`. `wowdps-gui [--overlay]` takes no source flags —
it cannot tail. `--overlay` is single-instance: a new launch evicts the running one
(unversioned takeover socket `overlay.sock` beside the daemon socket, so it works
across builds); plain windows may multiply freely.

## Dependencies
model: zero-dep. proto + daemon: stdlib only. core: stdlib only. tui: ratatui +
crossterm. gui: iced + iced_layershell + serde/toml. Everything else stdlib unless
justified and signed off. No chrono (hand-parse the timestamp), no tokio (threads +
channels), no serde outside the gui.

## Fixture (owner: validator)
`fixtures/sample.txt` — synthetic advanced-format log, 2 encounters (one kill, one wipe)
+ trash, 3 players + 1 pet, covering every modeled event type. Expected totals in
`fixtures/sample.expected.md` (hand-computed, independent of the parser).
`fixtures/corrupt.txt` — mutated copy for the negative control.
