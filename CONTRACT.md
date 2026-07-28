# Module contract (coordinator-owned; changes require coordinator sign-off)

Single binary crate `wowdps` with library-style modules. `tui` builds against these
signatures; `core` implements them. Field lists may grow; names/shapes below are the
agreed interface.

## src/parser.rs (owner: core)

```rust
pub struct LogLine { pub ts_ms: i64, pub event: Event }   // ts_ms: unix-ish ms, monotonic within a file

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
    Other,                       // recognized-as-log-line but not modeled; never an error
}

pub struct Spell { pub id: u32, pub name: String, pub school: u32 }
pub enum AuraType { Buff, Debuff }

/// None for blank/malformed lines. Unknown events => Some(LogLine{event: Other, ..}).
pub fn parse_line(line: &str) -> Option<LogLine>;
```

## src/meter.rs (owner: core)

```rust
pub struct Meter { /* feeds lines, owns segments */ }
impl Meter {
    pub fn new() -> Self;
    pub fn feed(&mut self, line: LogLine);
    pub fn segments(&self) -> &[Segment];          // history, oldest first; last = live/current
    pub fn current_index(&self) -> usize;
}

pub enum SegmentKind { Encounter, Trash }
pub struct Segment {
    pub kind: SegmentKind,
    pub name: String,              // encounter name, or "Trash"
    pub start_ms: i64,
    pub end_ms: Option<i64>,       // None while live
    pub success: Option<bool>,
}
impl Segment {
    pub fn duration_ms(&self, now_ms: i64) -> i64;
    /// Rows for a view, sorted desc by amount. pct is of view total.
    pub fn rows(&self, view: View) -> Vec<Row>;
    /// Drilldown for one player: (by-spell rows, by-target rows) for the view.
    pub fn breakdown(&self, player_guid: &str, view: View) -> (Vec<Row>, Vec<Row>);
}

pub enum View { Damage, Healing, Interrupts, CrowdControl, Dispels, Deaths }
pub struct Row {
    pub key: String,               // player guid (meter) / spell or target name (breakdown)
    pub label: String,             // display name
    pub amount: u64,               // damage done, healing done, or event count
    pub extra: u64,                // overheal for Healing; overkill for Damage; else 0
    pub per_sec: f64,              // DPS/HPS; 0.0 for count views
    pub pct: f64,                  // 0..100 of view total
    pub class: Option<Class>,      // from COMBATANT_INFO specID; bars render in class color
}
```

Semantics (RULINGS R1-R6, binding for meter AND fixture expected values):
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
  Warcraft Logs; accepted, no grace window).
- R7 Duration semantics: Encounter segments = ENCOUNTER_START..ENCOUNTER_END exactly.
  Trash segments = FIRST..LAST combat event inside the segment (active combat time,
  like in-game meters) — never open..close, which counts idle time and deflates DPS.
- R5 Pet by-spell breakdown row label: "{spell} ({petName})".
- R6 Mid-log COMBAT_LOG_VERSION = hard boundary: close open segment, reset pet-owner map.
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
full replay — same segments, same `rows()`, same breakdowns, same classes — is gated by
fixture tests in the module.

```rust
pub struct SegmentMeta {
    pub kind: SegmentKind,
    pub name: String,              // encounter name, or "Trash"
    pub start_ms: i64,
    pub end_ms: Option<i64>,       // None only on the trailing open segment
    pub success: Option<bool>,
    pub duration_ms: i64,          // R7 semantics
    pub byte_range: (u64, u64),    // [start, end) file offsets of the slice
    pub seeds: Vec<(u64, u64)>,    // earlier SPELL_SUMMON/COMBATANT_INFO/VERSION lines
}
pub struct Index {
    pub segments: Vec<SegmentMeta>,   // closed, oldest first
    pub open: Option<SegmentMeta>,    // trailing in-progress segment, if any
    pub live_offset: u64,             // where the live tail starts emitting lines
    pub scanned: u64,
}
pub fn scan<R: Read>(reader: &mut R) -> Index;
/// Seed lines first, then the slice: feeding these through a fresh Meter
/// reproduces the segment (incl. cross-segment pet ownership and classes).
pub fn load_segment(path: &Path, meta: &SegmentMeta) -> io::Result<Vec<String>>;
```

## src/tail.rs, src/app.rs, src/ui.rs (owner: tui)

- `tail.rs`: `Source` that yields events for (a) `--file <path>` or (b) the newest
  `WoWCombatLog*.txt` in `--logs <dir>`, following growth and rotating to a newer file
  when one appears. Polling (~200ms) is fine; no notify dependency. On open/rotate it
  emits `Switched`, then `Index { index, file_age_ms }` (one structural scan of the
  file), then `Lines` starting at the index's `live_offset` — history is never replayed
  line by line.
- `app.rs`: two screens. `Screen::List` is the startup segment browser over the index
  (plus the live meter's own segments); `Screen::Meter` is the meter/drilldown. An open
  trailing segment in a file younger than ~10s at scan time means a fight is in
  progress: startup skips the list and lands on the live meter. Opening an indexed
  segment sets a `load_request`; `main.rs` services it (`load_segment` +
  `install_loaded`, FIFO cache of 8 parsed segments) between frames.
- Keybinds: list — `j/k`/arrows move, `Enter` opens the segment, `q` quit. Meter —
  `d/h/i/c/x/K` views (damage/heal/interrupt/cc/dispel/deaths; capital K — lowercase k moves), `[`/`]` cycle
  segments (lazy-loading as needed), `Enter` drilldown on selected row, `Esc` closes
  the drilldown or, with none open, returns to the list, `j/k` or arrows move
  selection, `q` quit.
- CLI: `wowdps --file <log>` | `wowdps --logs <dir>` (default: built-in Steam proton path).

## Dependencies
ratatui + crossterm approved. Everything else stdlib unless justified in your status
file and signed off. No chrono (hand-parse the timestamp), no tokio (threads + channels),
no serde (not needed).

## Fixture (owner: validator)
`fixtures/sample.txt` — synthetic advanced-format log, 2 encounters (one kill, one wipe)
+ trash, 3 players + 1 pet, covering every modeled event type. Expected totals in
`fixtures/sample.expected.md` (hand-computed, independent of the parser).
`fixtures/corrupt.txt` — mutated copy for the negative control.
