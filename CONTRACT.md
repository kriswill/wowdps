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
    CombatantInfo  { guid: String, faction: u32 },   // faction = arena SIDE inside a match (R13)
    Damage { src: Unit, dst: Unit, spell: Option<Spell>, amount: u64, overkill: i64, absorbed: u64, critical: bool, periodic: bool },
    Heal   { src: Unit, dst: Unit, spell: Spell, amount: u64, overheal: u64, absorbed: u64, critical: bool },
    Absorbed { src: Unit, dst: Unit, absorber: Unit, spell: Option<Spell>, absorb_spell: Spell, amount: u64 },
    Interrupt { src: Unit, dst: Unit, spell: Spell, interrupted_spell: Spell },
    AuraApplied { src: Unit, dst: Unit, spell: Spell, aura_type: AuraType }, // Buff | Debuff
    AuraRemoved { src: Unit, dst: Unit, spell: Spell, aura_type: AuraType }, // v13: closes marker spans only
    Dispel { src: Unit, dst: Unit, spell: Spell, dispelled_spell: Spell },
    Cast { src: Unit, spell: Spell },   // R12: SPELL_CAST_SUCCESS; item markers only
    Summon { owner: Unit, pet: Unit },
    Death  { unit: Unit },
    ZoneChange         { map_id: u32, name: String, difficulty: u32 },  // R10; difficulty 0 = open world
    ChallengeModeStart { map_id: u32, key_level: u32 },                 // R10
    ChallengeModeEnd   { map_id: u32, success: bool },                  // R10
    ArenaMatchStart    { map_id: u32, match_type: String },             // R13
    ArenaMatchEnd      { winning_team: u32 },                           // R13
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
    pub arena: bool,               // R13: an arena match — success means WIN/LOSS
    pub noise: bool,               // R13: post-match arena tail — never listed, even live
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
    /// Rows for a view: friendly team first, then the enemy team (R13), each
    /// sorted desc by amount (Deaths: first-death order, R9). pct is of view total.
    pub fn rows(&self, view: View) -> Vec<Row>;
    /// Drilldown for one player: (by-spell rows, by-target rows) for the view.
    /// Deaths: (recap timeline newest-first, attacker totals) instead (R9).
    pub fn breakdown(&self, player_guid: &str, view: View) -> (Vec<Row>, Vec<Row>);
    /// R12: the player's damage on a fixed grid plus their item markers,
    /// both relative to this segment's start. Pets fold into their owner.
    pub fn timeline(&self, player_guid: &str) -> Timeline;
    pub fn heal_timeline(&self, player_guid: &str) -> Timeline;  // v14: R2 amounts, same grid/marks
    /// v16: one ability's damage on the same grid — keyed by the by-spell
    /// row's `key` ("spell" or "spell\0petName"), so client and meter agree
    /// on identity by construction. Damage only (the sparse per-spell series
    /// records nothing else); marks are the player's.
    pub fn spell_timeline(&self, player_guid: &str, spell_key: &str) -> Timeline;
    /// v17: who the ability landed on — per-target rows for one spell, keyed
    /// like `spell_timeline`, sorted desc; `pct` is of the SPELL's own total
    /// and rows wear its school. Works for every view (heals list recipients).
    pub fn spell_targets(&self, player_guid: &str, spell_key: &str, view: View) -> Vec<Row>;
    /// R12/v12: the per-spell table over a time window (`lo..hi` ms from the
    /// segment start; `None` = whole fight, and then it agrees with
    /// `breakdown` exactly — same fold, same labels, same tallies, because
    /// the sparse per-spell series rides the same `record` call). Returns the
    /// player's windowed total Row alongside (`per_sec` over the window).
    pub fn compare_spells(&self, player_guid: &str, range: Option<(i64, i64)>)
        -> (Row, Vec<Row>);
}

pub enum ItemKind { Trinket, Potion, Flask, Food, Consumable }   // R12
pub enum MarkKind { TrinketUse, TrinketProc, Consumable,
                    External }         // R12; External is v13
pub struct Mark { pub at_ms: i64, pub kind: MarkKind, pub label: String,
                  pub spell_id: u32,   // v12: for client-side icon lookup
                  pub dur_ms: i64 }    // v13: aura applied→removed; 0 = unknown
pub struct Timeline {                                            // R12
    pub bucket_ms: u32,          // 1000
    pub buckets: Vec<u64>,       // damage in [i*bucket_ms, (i+1)*bucket_ms)
    pub marks: Vec<Mark>,
}
impl Timeline {
    pub fn rolling_dps(&self, window_ms: u32) -> Vec<f64>;  // centred, end-clamped
    pub fn cumulative(&self) -> Vec<u64>;
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
    pub spell_id: u32,             // by-spell rows: the id behind the label (first-seen when ranks
                                   // share a name); 0 elsewhere. Client-side icon lookup (v9).
    pub enemy: bool,               // R13, meter rows: hostile side of an arena match, from the
                                   // unit-flags reaction bit; only in arena segments — never in
                                   // world PvP; false on breakdown rows (v10).
    pub school: u32,               // v15, by-spell rows: the spell's school bitmask as logged
}                                  // (1 Physical … 64 Arcane, combos OR; swings are Physical);
                                   // first-seen per label like spell_id; 0 elsewhere. Bars tint
                                   // by school in the GUI drilldown.
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
- R5 Pet by-spell breakdown row label: "{spell} ({petName})", aggregated by pet
  NAME — swarm specs (Army of the Dead, Wild Imps) summon dozens of same-named
  instances per fight, and a row per instance buries the drill; instances of one
  name merge into one row, differently named pets stay separate.
- R6 Mid-log COMBAT_LOG_VERSION = hard boundary: close open segment, reset pet-owner
  map, and SUSPEND the open visit (R10) — a mid-run /reload writes a version line
  with the key still in progress, and the ZONE_CHANGE the game re-fires right after
  resumes the visit; a seam elsewhere closes it at the next ZONE_CHANGE as usual.
- R8 Class/spec inference: outside instances COMBATANT_INFO never fires, so a player's
  class (and, when the spell is unique to one specialization, spec) is inferred from
  player-sourced spell events — Damage/Heal/Interrupt/Dispel/AuraApplied via `src`,
  SPELL_ABSORBED via the absorbing shield's caster — against the generated table
  `core/src/class_spells.rs` (spell id → class/spec; built by `tools/gen-class-spells.sh`
  from the local install's client DB2s: class skill lines + SpecializationSpells + trait trees;
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
  entering a different instance closes it. A KEYED visit resumes on map_id alone:
  the game re-fires ZONE_CHANGE mid-run (reloads, reconnects) carrying the keystone
  difficulty instead of the one stamped at the door, and that must not split the
  run (or its END is orphaned onto an unkeyed visit and ignored). Every CHALLENGE_MODE_START on the current
  visit's map is a visit boundary — the dungeon resets and the key's clock starts with
  the countdown, not at the door: the visit (and any open trash) closes and a fresh
  KEYED visit opens, so pre-key activity inside the instance (readiness heals, an
  earlier key) never joins the run's Overall. CHALLENGE_MODE_END counts
  only for a keyed visit (the zeroed reset the game fires on entry, before any START,
  is ignored) and sets `completed` from its success flag and `official_ms` from its
  totalMs field — the game's own run time, death penalties included. The END's
  success flag only means "completed" (it is 1 even in overtime), so the outcome a
  keyed visit REPORTS (`Visit::verdict`, shown as segment `success`) is the TIMED
  verdict against the dungeon's par timer (generated MapChallengeMode table keyed by
  START's challengeID, `keystone_timers.rs`): `official_ms <= par` once the END
  fired; before it, a run already past par reports failed — OVER shows the moment
  the timer elapses (up to 15s per death late, since live clocks carry no death
  penalties); an abandoned keyed run (END success 0) is failed; unknown challengeID
  falls back to the END flag. Segments opened while zoned
  in carry the visit's ordinal — that ordinal is the instance id associated with all
  counters. The visit's OVERALL (`Meter::overall`) is a synthetic
  `SegmentKind::Overall` segment: every member's counters merged (tallies sum;
  identity maps union, later member wins; death order first-occurrence across
  members; each player's latest recap wins), duration = the SUM of member durations
  (R7 applied per member, an open member cut at its last combat event), success =
  `completed`, name = `Visit::display_name()`. EXCEPT: a KEYED visit's Overall clock
  is the key timer, not combat time — `official_ms` once the END fired (exact, and
  frozen thereafter), otherwise wall clock from `start_ms + KEY_COUNTDOWN_MS` (10s:
  the in-game timer starts when the activation countdown ends) to `end_ms`/now,
  clamped ≥ 0. Live estimates therefore lag the in-game timer by 15s per death
  until the END corrects them. A keyed Σ row's per_sec is over this key clock —
  run DPS, not combat DPS. Live and lazy paths both build the
  Overall by merging members, so index-then-lazy equals full replay by construction.
  A scan cut mid-visit splits the visit at `live_offset`: the `open_visit` prefix
  carries only members closed before it — bytes and clock — and the open member
  belongs exclusively to the live tail (which replays it from its first line), so
  merging prefix + live counts every member exactly once.
  ZONE_CHANGE / CHALLENGE_MODE_* lines are SEED lines: replaying them ahead of any
  slice (or the live tail) reconstructs the visit table with file-consistent
  ordinals everywhere. In the combined segment list the Overall row precedes its
  visit's first member, and it exists only once the visit has a member.
- R11 Meaningful segments. The combat log records the whole neighborhood, so world
  Trash can consist entirely of NPC-vs-NPC noise or out-of-combat topping-off heals.
  A segment is WORTH A LIST ROW (`Segment::counts`, mirrored by the scanner into
  `SegmentMeta::counts`) iff it is an Encounter, its enemy tally is non-empty (a
  friendly damage event landed on a hostile — the same tally that names pulls), a
  player damaged ANOTHER player (duels, world PvP; self-damage excluded or every
  Blood DK ride would count), or a player died in it (the recap must survive). A
  live segment always surfaces — the meter still tracks world healing while it
  happens — and one that closes without counting is dropped from the daemon's list
  (both live and indexed paths); it still exists internally: ids stay positional,
  parity is over ALL segments, and Σ overalls merge every member of the visit
  regardless. A Σ row itself is listed only in front of a visible member — a visit
  whose every member was filtered leaves no dangling Σ-only block.
- R12 Player comparison: timelines and item markers. Every segment additionally
  keeps, per acting guid, damage bucketed on a fixed 1s grid anchored at
  `start_ms` (`Segment::timeline`, pets resolved onto their owner exactly like
  `rows`/`breakdown`; bounded by `MAX_BUCKETS` so a corrupt clock costs a clamp,
  not an allocation) — and, v14, effective healing (R2 amounts) on the same
  grid in its own series (`Segment::heal_timeline`, same pet folding, same
  markers), so the Healing drilldown graphs without touching the damage
  curve — and per PLAYER guid a bounded list (`MARK_CAP = 256`) of
  ITEM MARKERS. A marker's spell is classified by the generated table
  `core/src/item_spells.rs` (spell id → `ItemKind`; built by
  `tools/gen-item-spells.sh` from the local install's Item / ItemEffect /
  ItemXItemEffect tables, with `SpellEffect.EffectTriggerSpell` chased two levels
  out of trinket effects so proc buffs — which are never the item's own listed
  spell — are covered). `class_spells` WINS that lookup: the chase is generous
  and also claims ordinary class spells (a trinket that procs a free Fireball
  lists Fireball), which must never draw an item marker. A `Cast`
  (SPELL_CAST_SUCCESS) by a player marks `TrinketUse` for a trinket spell and
  `Consumable` for anything else; a Buff `AuraApplied` on a player marks
  `TrinketProc` — but only for trinkets, and only when no cast of that same
  spell by that player precedes it within 2s (an on-use trinket's own buff is
  its use, not a second proc); the same proc re-applying within 500ms is one
  proc, since trinkets refresh their buff as it stacks. v13 SPANS: a Buff
  `AuraRemoved` on a player closes the newest still-open mark of that spell,
  giving it `dur_ms` (aura applied→removed; unknown stays 0 and draws no
  span), and a Buff re-applying while a mark of that spell is still OPEN is
  a refresh, not a new mark — the open span keeps running. v13 EXTERNALS:
  spells in the CURATED `EXTERNAL_BUFFS` list (the Bloodlust family + Power
  Infusion) mark `MarkKind::External` when the buff LANDS on a player —
  checked before the class-spells veto, which would otherwise eat Power
  Infusion; the list is deliberately hand-picked so persistent raid buffs
  (Arcane Intellect, Mark of the Wild) can never clutter a graph. Casts and
  aura bookkeeping NEVER open or extend a segment (scanner lockstep, like R8/R9),
  and marker state is segment-local, so lazy loading reproduces timelines and
  markers exactly. `Cast` is deliberately NOT an R8 class-inference source —
  R8's sources are fixed, and widening them would move fixture expectations.
  Buckets and markers merge on `absorb` (R10): member curves shift by
  `(other.start_ms - self.start_ms)/bucket_ms`, so a visit's Overall spans the
  visit's wall clock. Markers are stored absolute and rebased by `timeline()`.
  The comparison itself is a CLIENT concern: `ClientState` holds at most two
  picked players, a third pick replaces the older, and `Screen::Compare` is
  reachable only with BOTH picked — a half-made pair keeps the meter up.
  Segment navigation (`[`/`]`, list-position jumps, return-to-live) never
  breaks an open comparison: the pair sticks and the new segment's sides are
  requested for it; only Back/right-click (or unpicking) closes it. The
  graph mode (`GraphMode::Dps` rolling / `Total` cumulative) is purely local:
  both curves come out of the buckets already in hand, so toggling never
  round-trips.
- R13 Arena matches. Arenas zone in with ZONE_CHANGE difficulty 0, so R10 never sees
  them: no visit opens, and without this ruling a match records as anonymous Trash
  named after the most-hit enemy pet. ARENA_MATCH_START (mapID, matchType)
  therefore opens an `Encounter`-kind segment — closing
  whatever was open, exactly like ENCOUNTER_START — named
  `"{zone} ({matchType})"` from the LAST ZONE_CHANGE's name at ANY difficulty
  (`Meter::last_zone`, mirrored by the scanner and persisted in
  `ScanState::last_zone` so a checkpoint resume between zone-in and gates still
  names the match; a log begun mid-match falls back to bare "Arena").
  THE VERDICT: ARENA_MATCH_START's trailing teamID is a dead constant 0 (verified
  live), so the HOME side comes from the match's own COMBATANT_INFO lines — field
  2 ("faction") is the player's arena side inside a match, and the game re-fires
  the infos right after the START. Factions are MATCH-LOCAL state; the home side
  resolves at the first friendly-flagged (reaction 0x10) player source of a
  damage event (every friendly shares one side, so resolution order cannot change
  the answer — which is what lets meter and scanner stay in lockstep without
  identical iteration order). ARENA_MATCH_END closes the segment with
  `success = (winningTeam == home)` — or verdict-less if the home side never
  resolved — so the overlay's kill/wipe colors read as win/loss with no extra
  wire fields. Encounter kind buys the
  rest: R7 clocks the match START..END (dampening lulls longer than the trash gap
  cannot split it), R11 always counts it, and gate-prep activity before the START
  stays behind in (non-counting) Trash. All arena state is match-local, held only
  while the match's segment is open: a stray END with no START closes nothing, and
  a mid-match COMBAT_LOG_VERSION seam (R6) drops it, orphaning the match's END —
  which also keeps it out of checkpoints entirely. Solo Shuffle logs one
  START/END pair around all six rounds; rounds are not split (future work).
  THE TAIL IS NOISE: pets and DoTs keep hitting for the seconds between
  ARENA_MATCH_END and the teleport out, and that decided-arena combat opens a
  Trash segment flagged `noise` — it exists internally (ids stay positional,
  parity is over ALL segments) but NEVER earns a list row, not even while live
  (R11's live exception does not apply), never announces a `SegmentOpened`,
  and the daemon's Live cursor skips it, so the meter stays parked on the
  finished match and its verdict. The window (`arena_over`) opens at any
  ARENA_MATCH_END — unconditionally, an END whose START predates the log
  still leaves us in a decided arena — and closes at any ZONE_CHANGE, the
  next ARENA_MATCH_START, or a version seam. It spans a region with no open
  segment, so it travels in `ScanState`; ARENA_MATCH_END lines are SEED lines
  so a lazy load of the tail reproduces the flag.
  Gated by `fixtures/arena.txt` + `tests/arena.rs` (replay semantics, scanner
  parity, lazy-load parity, checkpoint resumption).
  TEAMS: enemy players earn meter rows like anyone else (they are `Player-`
  GUIDs), so every meter row carries `enemy` — the unit-flags reaction bit
  (0x40 Hostile), set ONLY in `arena` segments (hostile-flagged players in
  the open world — war mode, duels — never split the chart into teams),
  segment-local like names/flags so lazy loads agree. Sorted
  views order rows (enemy, amount desc, label): the friendly team leads and
  the enemy team trails as one contiguous block, so a renderer splits the
  chart at the first `enemy` row (GUI surfaces draw a divider; the TUI reads
  enemy names in red; Deaths keeps pure death order and draws no divider).
  Breakdown rows are never `enemy`. Match segments carry `arena = true`
  (Segment, SegmentMeta, SegmentInfo, ListRow alike), and every surface words
  their `success` as the HOME TEAM'S outcome — WIN/LOSS, never KILL/WIPE.
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
    pub arena: bool,               // R13 mirror of Segment::arena
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
    pub last_zone: Option<String>,    // R13: last zone name seen, any difficulty
    pub arena_over: bool,             // R13: inside a decided arena at `offset`
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

## Wire protocol (owner: proto) — `PROTO_VERSION = 18`

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
`VisibilityChanged 0x04`, `Shutdown 0x05` (accepted pre-handshake, so `wowdps stop`
always works), `DiscardTrash 0x06` (R11: tombstone every closed out-of-instance
Trash segment for the daemon's lifetime — the live segment and visit members
survive — then broadcast the shrunken list; a daemon restart rescans everything). DaemonMsg `HelloAck 0x81`, `Snapshot 0x82`, `SegmentList 0x83`,
`SegmentOpened 0x84`, `LoadFailed 0x85`, `Status 0x86`, `SetVisible 0x87`,
`Fatal 0x88`, `CompareSnapshot 0x89`. A `Watch` carries a `Cursor` — `List`,
`Segment { SegmentRef (Live | Id), View, top_n, drill }`, or R12's
`Compare { SegmentRef, a: guid, b: guid, range: Option<(u32, u32)> }`
(answered with `CompareSnapshot`, carrying two
`CompareSide { guid, total: Row, spells: Vec<Row>, timeline }`;
the pair keeps the order given so the panes never swap under the user, and a
player absent from the segment yields an empty side, never an error. v12: a
`Some` range windows each side's `total` and `spells` to `lo..hi` ms from the
segment start — computed by `compare_spells`, no re-parse — while the
timelines stay whole (graph zoom is the client's own slice); the snapshot
echoes the range it answered, and renderers gate the zoomed view on the ECHO,
never on what they last asked for, so a stale in-flight snapshot cannot pair
full-fight tables with a zoomed graph) — and
replaces any prior
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
  `instance` — the R10 visit ordinal. v6: `SegmentInfo`/`ListRow` gained a trailing
  Option<(i64, i64, i64)> `pars_ms` — a keyed visit's (par, +2, +3) timers, set on
  Σ rows only, so clients render the tier and overtime detail. v7: ClientMsg gained
  `DiscardTrash 0x06`, empty body. v8 (R12): `Cursor` gained `Compare` (code 2)
  and DaemonMsg gained `CompareSnapshot 0x89`; a `Timeline` encodes as u32
  `bucket_ms` + Vec<u64> buckets + Vec<Mark>, and a `Mark` as i64 `at_ms` +
  u8 kind + string label. v9: `Row` gained a trailing u32 `spell_id`, 0 = none —
  the id behind a by-spell label, so clients can look up ability icons in the
  per-machine spell-icon cache without the wire carrying any art.
  v10 (R13): `Row` gained a trailing bool `enemy` — the player fought on the
  hostile side, so PvP charts can split the teams. v11 (R13): `SegmentInfo`
  and `ListRow` gained a trailing bool `arena`, so headers and list rows word
  a match's outcome as the home team's WIN/LOSS instead of KILL/WIPE.
  v12 (R12): `Cursor::Compare` gained a trailing Option<(u32, u32)> `range`
  and `CompareSnapshot` echoes it (after side `b`, before `source`); `Mark`
  gained a trailing u32 `spell_id`, 0 = none, for client-side ability icons
  on the graph's marker strip. v13 (R12): `MarkKind` gained `External`
  (code 3) and `Mark` a trailing i64 `dur_ms`, 0 = unknown, so renderers can
  wash the buff's active span and word an uptime. v14 (R12): `Breakdown` gained
  a trailing Option<Timeline> — the drilled view's OWN timeline on the same
  whole-fight grid a `CompareSide` carries: damage for Damage, effective
  healing for Healing (identical markers), absent for the count views — so
  the drilldown draws the comparison's graph without a second cursor;
  zooming it is the client's own slice and never round-trips. v15: `Row`
  gained a trailing u32 `school` — the spell's school bitmask exactly as the
  log wrote it (swings count as Physical), first-seen per label like
  `spell_id`, 0 on meter/by-target rows — so drilldown bars tint by damage
  type: the game's own school palette, component colors averaged for combo
  masks like Shadowflame. v16: the ABILITY drill — `Cursor::Segment` gained a
  trailing Option<String> `spell` (the drilled ability's by-spell key,
  meaningless without `drill`) and `Breakdown` a trailing Option<Timeline>
  `spell_timeline` (`Segment::spell_timeline`, present iff a spell is named
  and the view is Damage). The GUI draws it as the FOCUS curve in its school
  color over the player's ghosted line — one shared y-scale, so the ability
  reads as its share of the player — under a "Player ▸ Spell" breadcrumb and
  a stat strip wording what the by-spell row always carried: total, share,
  hits, crit, average hit, and overkill/overheal. Enter (or clicking a spell
  row) descends; Esc/right-click backs out ONE level — ability, drill, list;
  switching views closes the ability level (by-spell keys are view-local).
  The TUI words the same stats without a graph. v17: by-spell tallies grew
  per-TARGET maps (`Segment::spell_targets`) and `Breakdown` a trailing
  Option<Vec<Row>> `spell_targets`, present iff a spell is named — the
  ability pane lists who it landed on (school-tinted bars: name, hits,
  total, share of the spell), the stat strip gains a SCHOOL card (the
  game's combo names — Shadowflame, Chaos — else components joined with +),
  and the stat cells wear card boxes. v18: the COMPARISON drills too —
  `Cursor::Compare` gained a trailing Option<String> `spell` (ONE by-spell
  key applied to BOTH sides) and `CompareSide` a trailing Option<Timeline>
  `spell_timeline` (absent when that side never cast it). Clicking a spell
  row drills both panes: each shows the ability's stats and its focus curve
  over that side's ghost, on the pair's one shared y-scale; a side without
  the spell says so and keeps its own line. Esc/right-click backs out the
  ability FIRST, the pair second.)

Client state (owner: proto): `state::ClientState` holds screen/view/selection/drill
plus the cached last snapshot, and R12's comparison pair + graph mode;
`apply(Action)`/`on_msg(DaemonMsg)` return the
`ClientMsg`s to send. Held-key `Up`/`Down` clamps against the cache and never
round-trips. Keybinds (owner: clients): list — `j/k`/arrows move, `Enter` opens,
`q` quit. Meter — `d/h/i/c/x/K` views (capital K — lowercase k moves), `[`/`]`
cycle segments, `Enter` drilldown, `Esc` back (drilldown, then list), `q` quit.
R12 (GUI only; the TUI binds neither, so `Screen::Compare` is unreachable there)
— `v` picks/unpicks the selected player for the comparison, `g` toggles the
graph between rolling DPS and cumulative, `Esc` leaves the comparison, and a
RIGHT-CLICK on the body clears the pair (or a lone half-pick) and returns to
the meter — on the overlay, which has no keyboard, that is the only way back.
In both GUI surfaces the per-row CLASS ICON is the pick target and the bar
still drills: two hit areas, two questions.
v12 graph gestures: LEFT-DRAG on either comparison graph selects a time
window — both tables, totals and graphs re-answer for exactly that window
(`Cursor::Compare.range`; a sub-3px wander is a click, not a selection);
RIGHT-CLICK on a graph zooms back out to the whole fight (the canvas captures
it, so it never falls through and closes the comparison); item markers wear
their ability icon in a strip along the graph's top edge, and HOVERING one
highlights every use of that item on BOTH graphs. v14: the GUI drilldown
(Damage and Healing views — each graphs its own metric, worded "dps"/"hps")
draws the same single-player graph under its panes — `g`
toggles the curve there too, drag zooms (client-side only, the timeline is
whole), and right-click on the graph zooms back out (captured, so it never
falls through to close the drill). A hovered marker's numbers take over the
LEGEND row (name, kind × uses, uptime and window share) instead of drawing a
panel over the curve. On a Σ drilldown the graph underlines its ENCOUNTER
LANE — green bars along the bottom edge spanning the visit's boss fights,
computed client-side from the segment list (`ClientState::encounter_spans`:
Encounter members of the watched visit, `start_ms` rebased onto the
Overall's — no wire change). v13: markers with a known
`dur_ms` wash their active span under the curve, and the hovered graph draws
an info panel — name, kind, use count, total uptime and its share of the
displayed window.

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

CLI (git-style subcommands): `wowdps [--file|--logs]` (TUI client; source conflict
with a running daemon is a hard error naming both), `wowdps gui [--file|--logs]`,
`wowdps daemon [--linger] [--file|--logs]`, `wowdps status`, `wowdps stop`,
`wowdps help`. Any other first word dispatches externally: `wowdps <cmd> [args…]`
execs `wowdps-<cmd>` with the tail verbatim, preferring a sibling of the running
binary (same build) over `$PATH` — `wowdps extract …` runs `wowdps-extract`, and
the dev shells expose `tools/gen-*.sh` as `wowdps-gen-<name>` so `wowdps
gen-icons` works in a checkout. The retired flag spellings (`--daemon`, `--gui`,
`--stop`, `--status`) error, naming the subcommand. `wowdps-gui [--overlay]`
takes no source flags — it cannot tail. `--overlay` is single-instance: a new
launch evicts the running one (unversioned takeover socket `overlay.sock` beside
the daemon socket, so it works across builds); plain windows may multiply freely.

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
`fixtures/instance.txt` — R10: keystone visits, suspend/resume, city combat between.
`fixtures/arena.txt` — R13: three skirmishes (win, loss, live-at-EOF) with prep
healing and a dampening-length lull, gated by `tests/arena.rs`.
