# Module contract (coordinator-owned; changes require coordinator sign-off)

Workspace around a client/server split. Field lists may grow; names, shapes and
rulings below are the agreed interface.

- `wowdps-model` — domain types only (`View`, `Row`, `Class`, `Spec`, `SegmentKind`,
  `SegmentId`, `SegmentInfo`, `ListRow`, `Screen`, `Pane`, `Drill`, `Action`, `fmt`).
  Zero dependencies, no I/O, no parser.
- `wowdps-core` — the engine: `parser`, `meter`, `index`, `tail`. Re-exports model.
  Only the daemon runs it.
- `wowdps-proto` — wire codec (`wire`, `msg`), client library (`client`), client-side
  state machine (`state::ClientState`). Depends on model only — a crate linking proto
  cannot parse a combat log even by accident.
- `wowdps-daemon` — the headless daemon: one tail/index/meter pipeline serving every
  client over a unix socket, plus the game watcher and overlay supervisor.
- Binaries: `wowdps` (daemon + launcher + TUI client; links core transitively, but
  `crates/tui/src` never names engine modules — gated by `tests/no_engine.rs`),
  `wowdps-gui` (window + `--overlay`; pure client, deps model + proto only),
  `wowdps-mcp` (MCP stdio server; pure client, model + proto only).

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
    AuraRemoved { src: Unit, dst: Unit, spell: Spell, aura_type: AuraType }, // closes marker spans only (R12)
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
    pub fn heal_timeline(&self, player_guid: &str) -> Timeline;  // R2 amounts, same grid/marks
    /// One ability's damage on the same grid — keyed by the by-spell row's
    /// `key` ("spell" or "spell\0petName"), so client and meter agree on
    /// identity by construction. Damage only; marks are the player's.
    pub fn spell_timeline(&self, player_guid: &str, spell_key: &str) -> Timeline;
    /// Who the ability landed on — per-target rows for one spell, keyed like
    /// `spell_timeline`, sorted desc; `pct` is of the SPELL's own total and
    /// rows wear its school. Works for every view (heals list recipients).
    pub fn spell_targets(&self, player_guid: &str, spell_key: &str, view: View) -> Vec<Row>;
    /// R12: the per-spell table over a time window (`lo..hi` ms from segment
    /// start; `None` = whole fight, and then it agrees with `breakdown`
    /// exactly — same fold, same labels, same tallies). Returns the player's
    /// windowed total Row alongside (`per_sec` over the window).
    pub fn compare_spells(&self, player_guid: &str, range: Option<(i64, i64)>)
        -> (Row, Vec<Row>);
}

pub enum ItemKind { Trinket, Potion, Flask, Food, Consumable }   // R12
pub enum MarkKind { TrinketUse, TrinketProc, Consumable, External }  // R12
pub struct Mark { pub at_ms: i64, pub kind: MarkKind, pub label: String,
                  pub spell_id: u32,   // client-side icon lookup
                  pub dur_ms: i64 }    // aura applied→removed; 0 = unknown, draws no span
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
                                   // share a name); 0 elsewhere. Client-side icon lookup.
    pub enemy: bool,               // R13, meter rows: hostile side of an arena match; only in arena
                                   // segments, never world PvP; false on breakdown rows
    pub school: u32,               // by-spell rows: school bitmask as logged (1 Physical … 64
}                                  // Arcane, combos OR; swings Physical); first-seen per label
                                   // like spell_id; 0 elsewhere. GUI drill bars tint by school.
```

## Rulings (binding for meter AND fixture expected values)

Only players (and their pets, folded onto them) get meter rows.

- **R1 Damage.** Row amount = per-event `amount + absorbed-field` (absorbed-by-shield
  damage counts as damage done, meter convention); extra = overkill clamped ≥ 0.
  Count SWING_DAMAGE only (SWING_DAMAGE_LANDED → Other); `*_SUPPORT` → Other;
  DAMAGE_SPLIT excluded from offensive totals.
- **R2 Healing.** Row amount = effective healing (amount − overheal); extra = overheal.
  SPELL_ABSORBED credits the ABSORBER with healing (no overheal component). The
  damage-event absorbed field never contributes to any healing number. Stagger /
  cheat-death self-absorbs (114556, 31850, 31230, 115069) excluded from healing.
- **R3 Absorb attribution.** SPELL_ABSORBED is the sole source for absorb-as-healing;
  the damage-event absorbed field is the sole source for absorb-as-damage. Different
  views, different actors — no double count.
- **R4 Segment boundaries.** ENCOUNTER_START opens an Encounter segment (closing any
  open one); ENCOUNTER_END closes it exactly (known ~1–3% DoT-tail divergence vs
  Warcraft Logs; accepted, no grace window). Combat outside encounters accrues to a
  Trash segment; a new Trash segment starts after >60s with no combat events. Every
  ZONE_CHANGE also closes the open Trash segment — a teleport is a hard location
  break; without it pre-instance trash would bleed into (and out of) a visit (R10).
- **R5 Pets.** Damage/heals by a unit summoned by a player (SPELL_SUMMON or
  advanced-field ownerGUID) count toward the owner — never separate meter rows.
  Breakdown by-spell label: "{spell} ({petName})", aggregated by pet NAME — swarm
  specs summon dozens of same-named instances per fight; one row per name, and
  differently named pets stay separate.
- **R6 Version seam.** Mid-log COMBAT_LOG_VERSION is a hard boundary: close the open
  segment, reset the pet-owner map, SUSPEND the open visit (R10) — a mid-run /reload
  writes a version line with the key still in progress, and the ZONE_CHANGE the game
  re-fires right after resumes the visit; a seam elsewhere closes it at the next
  ZONE_CHANGE as usual.
- **R7 Duration.** Encounter segments = ENCOUNTER_START..ENCOUNTER_END exactly. Trash
  segments = FIRST..LAST combat event inside the segment (active combat time, like
  in-game meters) — never open..close, which counts idle time and deflates DPS.
- **R8 Class/spec inference.** Outside instances COMBATANT_INFO never fires, so class
  (and, when the spell is unique to one specialization, spec) is inferred from
  player-sourced spell events — Damage/Heal/Interrupt/Dispel/AuraApplied via `src`,
  SPELL_ABSORBED via the absorbing shield's caster — against the generated table
  `core/src/class_spells.rs` (spell id → class/spec; `tools/gen-class-spells.sh`
  from the local install's DB2s: class skill lines + SpecializationSpells + trait
  trees; multi-class spells excluded, class-wide spells carry no spec). Inference is
  SEGMENT-LOCAL: it writes only the open segment, never the carried-forward maps, so
  lazy loading reproduces it exactly. COMBATANT_INFO is authoritative — it overwrites
  inference and is the only class/spec source that persists across segments.
  Inference never opens or extends a segment (scanner lockstep). The source list is
  FIXED — widening it (e.g. with `Cast`) would move fixture expectations.
- **R9 Deaths & the recap.** `rows(Deaths)` lists players in FIRST-death order,
  amount = 1 per death. Each Segment keeps a bounded per-player ring
  (`RECAP_CAP = 32`) of recent events on that player: damage hits (amount = the
  per-event `amount` alone — the absorbed part never touched their health and
  appears as its own gain entry) and gains (heals at effective value with overheal
  in extra; consumed absorbs via SPELL_ABSORBED). UNIT_DIED drains the ring into
  that player's recap — latest death wins — so `breakdown(guid, Deaths)` returns
  (timeline newest-first, killing blow with its overkill-in-extra leading; attacker
  totals sorted desc, gains excluded, source-less damage bucketed under its spell
  name). Timeline rows carry `hp`/`gain`; labels "{spell} ({source})", spell alone
  when the source unit is nil. HP comes from the line's own advanced block when it
  describes the victim, else back-fills onto the newest HP-less entry from the next
  advanced line describing them within 1s (SWING_DAMAGE → its LANDED twin;
  SPELL_ABSORBED → the paired damage line). Health reports and recap bookkeeping
  never open or extend a segment; the ring is segment-local (lazy-load parity).
- **R10 Instance visits & the per-visit Overall.**
  - A ZONE_CHANGE with difficulty ≠ 0 opens a *visit* (map_id + difficulty + zone
    name); ordinals index the file's visit table in order. Zoning out (difficulty 0)
    SUSPENDS it — segments recorded outside carry no visit — re-entering the same
    (map_id, difficulty) resumes it; a different instance closes it. A KEYED visit
    resumes on map_id alone: mid-run re-fires (reloads, reconnects) carry the
    keystone difficulty instead of the one stamped at the door, and a split would
    orphan the END onto an unkeyed visit.
  - Every CHALLENGE_MODE_START on the current visit's map is a visit boundary — the
    dungeon resets and the key's clock starts with the countdown, not at the door:
    the visit (and any open trash) closes and a fresh KEYED visit opens, so pre-key
    activity never joins the run's Overall.
  - CHALLENGE_MODE_END counts only for a keyed visit (the zeroed reset fired on
    entry, before any START, is ignored); it sets `completed` from its success flag
    and `official_ms` from its totalMs — the game's own run time, death penalties
    included. The success flag only means "completed" (1 even in overtime), so a
    keyed visit's REPORTED outcome (`Visit::verdict`, shown as segment `success`)
    is the TIMED verdict against the dungeon's par (generated `keystone_timers.rs`,
    MapChallengeMode keyed by START's challengeID): `official_ms <= par` once the
    END fired; before it, a run already past par reports failed — OVER shows
    the moment the timer elapses (up to 15s per death late — live clocks carry
    no death penalties); an abandoned keyed run
    (END success 0) is failed; unknown challengeID falls back to the END flag.
  - Segments opened while zoned in carry the visit's ordinal. The visit's OVERALL
    (`Meter::overall`) is a synthetic `SegmentKind::Overall` segment: members'
    counters merged (tallies sum; identity maps union, later member wins; death
    order first-occurrence across members; each player's latest recap wins),
    duration = SUM of member durations (R7 per member, an open member cut at its
    last combat event), success = `completed`, name = `Visit::display_name()`.
  - EXCEPT: a KEYED visit's Overall clock is the key timer, not combat time —
    `official_ms` once the END fired (exact, frozen), else wall clock from
    `start_ms + KEY_COUNTDOWN_MS` (10s — the in-game timer starts when the
    activation countdown ends) to `end_ms`/now, clamped ≥ 0. Live estimates lag
    the in-game timer by 15s per death until the END corrects them. A keyed Σ
    row's per_sec is over this key clock — run DPS, not combat DPS.
  - Live and lazy paths both build the Overall by merging members, so
    index-then-lazy equals full replay by construction. A scan cut mid-visit
    splits the visit at `live_offset`: the `open_visit` prefix carries only
    members closed before it — bytes and clock — and the open member belongs
    exclusively to the live tail (which replays it from its first line), so
    merging prefix + live counts every member exactly once.
  - ZONE_CHANGE / CHALLENGE_MODE_* lines are SEED lines: replaying them ahead of
    any slice (or the live tail) reconstructs the visit table with
    file-consistent ordinals everywhere. In the combined segment list the Overall
    row precedes its visit's first member, and exists only once the visit has a
    member.
- **R11 Meaningful segments.** The log records the whole neighborhood, so world
  Trash can be pure NPC-vs-NPC noise or out-of-combat heals. A segment is WORTH A
  LIST ROW (`Segment::counts`, mirrored by the scanner into `SegmentMeta::counts`)
  iff it is an Encounter, its enemy tally is non-empty (a friendly damage event
  landed on a hostile — the same tally that names pulls), a player damaged ANOTHER
  player (duels, world PvP; self-damage excluded), or a player died in it (the
  recap must survive). A live segment always surfaces; one that closes without
  counting is dropped from the daemon's list (live and indexed paths alike) but
  still exists internally: ids stay positional, parity is over ALL segments, and Σ
  overalls merge every member regardless. A Σ row is listed only in front of a
  visible member — a fully filtered visit leaves no dangling Σ-only block.
- **R12 Timelines, item markers & the comparison.**
  - Every segment keeps, per acting guid, damage bucketed on a fixed 1s grid
    anchored at `start_ms` (`Segment::timeline`; pets fold onto owners exactly
    like `rows`/`breakdown`; bounded by `MAX_BUCKETS` so a corrupt clock costs a
    clamp, not an allocation), and effective healing (R2 amounts) on the same grid
    in its own series (`heal_timeline`, same folding, same markers).
  - Per PLAYER guid, a bounded list (`MARK_CAP = 256`) of ITEM MARKERS. A marker's
    spell is classified by the generated `core/src/item_spells.rs` (spell id →
    `ItemKind`; `tools/gen-item-spells.sh` from Item / ItemEffect /
    ItemXItemEffect, with `SpellEffect.EffectTriggerSpell` chased two levels out
    of trinket effects so proc buffs — never the item's own listed spell — are
    covered). `class_spells` WINS that lookup: the chase is generous and also
    claims ordinary class spells, which must never draw an item marker.
  - A `Cast` (SPELL_CAST_SUCCESS) by a player marks `TrinketUse` for a trinket
    spell, `Consumable` for anything else. A Buff `AuraApplied` on a player marks
    `TrinketProc` — trinkets only, and only when no cast of that spell by that
    player precedes it within 2s (an on-use trinket's own buff is its use); the
    same proc re-applying within 500ms is one proc (buffs refresh as they stack).
  - SPANS: a Buff `AuraRemoved` on a player closes the newest still-open mark of
    that spell, setting `dur_ms` (unknown stays 0, draws no span); a Buff
    re-applying while a mark of that spell is OPEN is a refresh, not a new mark.
  - EXTERNALS: spells in the CURATED `EXTERNAL_BUFFS` list (Bloodlust family +
    Power Infusion) mark `External` when the buff LANDS on a player — checked
    before the class-spells veto (which would otherwise eat Power Infusion); the
    list is hand-picked so persistent raid buffs can never clutter a graph.
  - Casts and aura bookkeeping NEVER open or extend a segment (scanner lockstep,
    like R8/R9); marker state is segment-local, so lazy loading reproduces
    timelines and markers exactly. `Cast` is deliberately NOT an R8 source.
  - Buckets and markers merge on `absorb` (R10): member curves shift by
    `(other.start_ms − self.start_ms) / bucket_ms`, so a visit's Overall spans
    the visit's wall clock. Markers are stored absolute, rebased by `timeline()`.
  - The comparison itself is a CLIENT concern: `ClientState` holds at most two
    picked players, a third pick replaces the older, and `Screen::Compare` is
    reachable only with BOTH picked — a half-made pair keeps the meter up.
    Segment navigation (`[`/`]`, list jumps, return-to-live) never breaks an open
    comparison: the pair sticks and the new segment's sides are requested; only
    Back/right-click (or unpicking) closes it. Graph mode (rolling DPS /
    cumulative) is purely local — both curves come from buckets in hand.
- **R13 Arena matches.**
  - Arenas zone in with ZONE_CHANGE difficulty 0, so R10 never sees them; without
    this ruling a match records as anonymous Trash. ARENA_MATCH_START (mapID,
    matchType) opens an `Encounter`-kind segment — closing whatever was open,
    exactly like ENCOUNTER_START — named `"{zone} ({matchType})"` from the LAST
    ZONE_CHANGE's name at ANY difficulty (`Meter::last_zone`, mirrored by the
    scanner and persisted in `ScanState::last_zone` so a checkpoint resume between
    zone-in and gates still names the match; a log begun mid-match falls back to
    "Arena").
  - VERDICT: ARENA_MATCH_START's trailing teamID is a dead constant 0 (verified
    live), so the HOME side comes from the match's own COMBATANT_INFO lines —
    field 2 ("faction") is the player's arena side, re-fired right after the
    START. Factions are MATCH-LOCAL state; the home side resolves at the first
    friendly-flagged (reaction 0x10) player source of a damage event (every
    friendly shares one side, so resolution order cannot change the answer —
    which lets meter and scanner stay in lockstep without identical iteration
    order). ARENA_MATCH_END closes the segment with
    `success = (winningTeam == home)` — verdict-less if the home side never
    resolved — so kill/wipe colors read as win/loss with no extra wire fields.
  - Encounter kind buys the rest: R7 clocks the match START..END (dampening lulls
    longer than the trash gap cannot split it), R11 always counts it, and
    gate-prep activity before the START stays behind in (non-counting) Trash.
  - All arena state is match-local, held only while the match's segment is open:
    a stray END with no START closes nothing; a mid-match COMBAT_LOG_VERSION seam
    (R6) drops it, orphaning the match's END — which also keeps it out of
    checkpoints. Solo Shuffle logs one START/END pair around all six rounds;
    rounds are not split (future work).
  - THE TAIL IS NOISE: pets and DoTs keep hitting between ARENA_MATCH_END and the
    teleport out; that decided-arena combat opens a Trash segment flagged `noise`
    — it exists internally (ids positional, parity over ALL segments) but NEVER
    earns a list row, not even live (R11's live exception does not apply), never
    announces a `SegmentOpened`, and the daemon's Live cursor skips it, so the
    meter stays parked on the finished match and its verdict. The window
    (`arena_over`) opens at any ARENA_MATCH_END — unconditionally, an END whose
    START predates the log still leaves us in a decided arena — and closes at any
    ZONE_CHANGE, the next ARENA_MATCH_START, or a version seam. It spans a region
    with no open segment, so it travels in `ScanState`; ARENA_MATCH_END lines are
    SEED lines so a lazy load of the tail reproduces the flag.
  - TEAMS: enemy players earn meter rows like anyone else (`Player-` GUIDs), so
    every meter row carries `enemy` — the unit-flags reaction bit (0x40 Hostile),
    set ONLY in `arena` segments (hostile-flagged players in the open world — war
    mode, duels — never split the chart), segment-local like names/flags so lazy
    loads agree. Sorted views order rows (enemy, amount desc, label): the friendly
    team leads, the enemy team trails as one block, and a renderer splits the
    chart at the first `enemy` row (GUI surfaces draw a divider; the TUI reads
    enemy names in red; Deaths keeps pure death order, no divider). Breakdown
    rows are never `enemy`. Match segments carry `arena = true` (Segment,
    SegmentMeta, SegmentInfo, ListRow alike), and every surface words their
    `success` as the HOME TEAM'S outcome — WIN/LOSS, never KILL/WIPE.
  - Gated by `fixtures/arena.txt` + `tests/arena.rs` (replay semantics, scanner
    parity, lazy-load parity, checkpoint resumption).
- **R14 Talent dataset & import-string codec.** `tools/gen-talent-trees.sh` joins
  the install's Trait DB2 tables into `$XDG_DATA_HOME/wowdps/talents.json` — a
  per-machine cache like the icon bins (Blizzard-derived strings never enter the
  repo), deterministic per build. The ACTIVE tree per class comes from the class
  SkillLine (matched by display name, CategoryID 7) → SkillLineXTraitTree;
  TraitTreeLoadout alone also names retired and dev/test trees. Each tree's
  `nodeOrder` is every node id ascending — exactly the walk order of the in-game
  import string (serialization version 2: 6-bit LSB-first groups over the base64
  alphabet; header 8-bit version, 16-bit spec, 128-bit tree hash, zero = skip
  validation; per node selected(1) / purchased(1) / partially-ranked(1)+ranks(6) /
  choice(1)+entry-index(2); granted nodes stop after the purchased bit; the choice
  bit follows the node TYPE — Selection/SubTreeSelection — not the entry count).
  The mcp crate implements the codec from the dataset alone (stdlib file IO, no
  daemon round-trip); encode zero-fills the hash; a missing dataset is a
  tool-level error naming the generator. Gate: byte-identical decode→encode
  round-trip of a real exported string — the env-gated test
  `real_talent_string_round_trips_byte_identically` (crates/mcp/tests/server.rs;
  `WOWDPS_REAL_TALENT_STRING=C… cargo test -p wowdps-mcp -- --ignored
  real_talent`), run per patch alongside dataset regeneration; committed tests
  cover the same round-trip on a synthetic fixture.
- **R15 Count views & labels.** The Interrupts by-spell pane answers "what got
  kicked" — "{interrupted spell} ({interrupt ability})"; the CrowdControl pane
  answers "who got locked down" — "{cc spell} ({victim})". Meter-row counts are
  unchanged by these labels. The CrowdControl view counts AuraApplied debuffs
  whose spell is in a small built-in loss-of-control list (stuns, roots, incaps,
  fears — `const CC_SPELLS`/heuristic; exactness not gated).

## src/index.rs (owner: core)

Fast structural scan: segment boundaries + byte ranges, no per-event parsing, so a
300 MB+ log lists its segments in <1s. A segment's events are parsed only when
opened (`load_segment` + `Meter::feed`). The scanner mirrors `Meter::feed`'s
segmentation exactly; parity with a full replay — same segments, same `rows()`,
same breakdowns (recaps included), same classes/specs — is gated by fixture tests
in the module.

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

`Tailer` yields events for one file or the newest `WoWCombatLog*.txt` in a
directory, following growth and rotating to a newer file when one appears. Polling
(~200ms), no notify dependency. On open/rotate: `Switched`, then
`Index { index, file_age_ms }` (one structural scan — injectable via
`Tailer::with_scan`, where the daemon's index cache plugs in), then `Lines` from
the index's `live_offset` — history is never replayed line by line. `CaughtUp`
fires once when the backlog drains; `Lines` after it are fresh combat.

## Wire protocol (owner: proto) — `PROTO_VERSION = 18`

Transport: unix socket `$XDG_RUNTIME_DIR/wowdps/wowdps-v<PROTO_VERSION>.sock`
(fallback `/tmp/wowdps-<uid>/`, dir 0700, ownership verified). The version lives
in the socket *name*: version skew is structurally impossible — a new client
spawns its own daemon and the old one idle-exits.

Framing: `u32 len (LE) | u8 tag | body`, `len` covers tag+body, `MAX_FRAME`
16 MiB. Primitives: fixed-width LE integers, f64 as bits, bool as 0/1 byte,
string = u32 len + UTF-8, Option = presence byte, Vec = u32 count + items.
Decoding returns `Result` — truncation, bad tags, bad bools, bad UTF-8 and lying
counts are errors, never panics or attacker-sized allocations.

Messages (tags):

| ClientMsg | tag | DaemonMsg | tag |
|---|---|---|---|
| `Hello` | 0x01 | `HelloAck` | 0x81 |
| `Watch` | 0x02 | `Snapshot` | 0x82 |
| `GetStatus` | 0x03 | `SegmentList` | 0x83 |
| `VisibilityChanged` | 0x04 | `SegmentOpened` | 0x84 |
| `Shutdown` (pre-handshake OK, so `wowdps stop` always works) | 0x05 | `LoadFailed` | 0x85 |
| `DiscardTrash` (R11: tombstone every closed out-of-instance Trash segment for the daemon's lifetime — the live segment and visit members survive — then broadcast the shrunken list; a daemon restart rescans everything) | 0x06 | `Status` | 0x86 |
| | | `SetVisible` | 0x87 |
| | | `Fatal` | 0x88 |
| | | `CompareSnapshot` | 0x89 |

A `Watch` carries a `Cursor` — `List`; `Segment { SegmentRef (Live | Id), View,
top_n, drill, spell }`; or `Compare { SegmentRef, a, b, range, spell }` — and
replaces any prior cursor. The daemon pushes snapshots for exactly what is
watched, breakdown included when drilled. `Compare` is answered with
`CompareSnapshot` (two `CompareSide { guid, total: Row, spells: Vec<Row>,
timeline, spell_timeline }`s in the order given, so the panes never swap
under the user; a player absent from the segment yields an empty side, never an
error; a `Some` range windows each side's `total`/`spells` via `compare_spells`
while the timelines stay whole, and the snapshot ECHOES the range it answered —
renderers gate the zoomed view on the echo, never on what they last asked for, so
a stale in-flight snapshot cannot pair full-fight tables with a zoomed graph).
One standing exception: whenever the segment id table changes shape (a segment
appears, the file rotates), the daemon broadcasts a fresh `SegmentList` to every
session regardless of cursor — off-list navigation resolves neighbors through
that table, and `SegmentOpened` alone cannot keep it complete (the log arrives in
multi-minute flush bursts; a segment that opens *and* closes inside one batch
never announces itself).

Guarantees:

- `SegmentId`s are monotonic for the daemon's lifetime and never reused; after
  rotation a stale id resolves to `LoadFailed(Rotated | NotFound)`, never to
  another file's fight. A changed `source` on any snapshot means rotation:
  clients reset and re-`Watch`.
- Snapshot/list `seq` is per-session monotonic. Snapshots are idempotent: a
  lagging client is caught up by dropping stale ones (the client library
  coalesces to the newest per (segment, view)); control messages are ordered and
  never dropped.
- A snapshot whose segment is still being parsed carries the `loading_status`
  marker in `status` (empty placeholder rows); interactive clients paint it,
  request/response clients wait through it via `is_loading_status`.
- Encoded shapes are pinned by golden-byte tests in `crates/proto/tests/codec.rs`;
  changing any shape means bumping `PROTO_VERSION` (renaming the socket) and
  re-blessing them.

Version history (wire-shape changes only; added fields are trailing, new enum
variants take the next code):

| v | change |
|---|---|
| 2 | `Row` + u16 Blizzard specID (0 = none; raw id, unknown decodes to `None`) |
| 3 | `Row` + u64 `count`, u64 `crits` |
| 5 | `SegmentKind` + `Overall` (code 2); `SegmentInfo`/`ListRow` + Option<u32> `instance` (R10 ordinal) |
| 6 | `SegmentInfo`/`ListRow` + Option<(i64,i64,i64)> `pars_ms` (keyed (par,+2,+3), Σ rows only) |
| 7 | ClientMsg + `DiscardTrash 0x06` (empty body) |
| 8 | `Cursor` + `Compare` (code 2); DaemonMsg + `CompareSnapshot 0x89`; `Timeline` = u32 bucket_ms + Vec buckets + Vec<Mark>; `Mark` = i64 at_ms + u8 kind + string label |
| 9 | `Row` + u32 `spell_id` (0 = none; icon lookup in the per-machine cache) |
| 10 | `Row` + bool `enemy` (R13 team split) |
| 11 | `SegmentInfo`/`ListRow` + bool `arena` (R13 WIN/LOSS wording) |
| 12 | `Cursor::Compare` + Option<(u32,u32)> `range`, echoed by `CompareSnapshot` (after side `b`, before `source`); `Mark` + u32 `spell_id` |
| 13 | `MarkKind` + `External` (code 3); `Mark` + i64 `dur_ms` (0 = unknown) |
| 14 | `Breakdown` + Option<Timeline> — the drilled view's own timeline on the same whole-fight grid a `CompareSide` carries, identical markers (damage / effective healing; absent for count views) |
| 15 | `Row` + u32 `school` (bitmask as logged; swings Physical; first-seen per label; 0 on meter/by-target rows) |
| 16 | `Cursor::Segment` + Option<String> `spell` (ability drill key; meaningless without `drill`); `Breakdown` + Option<Timeline> `spell_timeline` (present iff spell named and view is Damage) |
| 17 | `Breakdown` + Option<Vec<Row>> `spell_targets` (present iff spell named) |
| 18 | `Cursor::Compare` + Option<String> `spell` (ONE key, BOTH sides); `CompareSide` + Option<Timeline> `spell_timeline` (absent when that side never cast it) |

## Client state & behavior (owner: proto; keybinds owner: clients)

`state::ClientState` holds screen/view/selection/drill, the cached last snapshot,
and the comparison pair + graph mode; `apply(Action)`/`on_msg(DaemonMsg)` return
the `ClientMsg`s to send. Held-key `Up`/`Down` clamps against the cache and never
round-trips.

Keybinds: list — `j/k`/arrows move, `Enter` opens, `q` quit. Meter — `d/h/i/c/x/K`
views (capital K — lowercase k moves), `[`/`]` cycle segments, `Enter` drilldown,
`Esc` back (drilldown, then list), `q` quit. GUI only (the TUI binds neither, so
`Screen::Compare` is unreachable there): `v` picks/unpicks the selected player for
the comparison, `g` toggles rolling DPS / cumulative, `Esc` leaves the comparison,
RIGHT-CLICK on the body clears the pair (or a lone half-pick) and returns to the
meter — on the overlay, which has no keyboard, that is the only way back. In both
GUI surfaces the per-row CLASS ICON is the pick target and the bar still drills:
two hit areas, two questions.

Drill navigation: `Enter` (or clicking a spell row) descends into the ability
level; `Esc`/right-click backs out ONE level — ability, drill, list; switching
views closes the ability level (by-spell keys are view-local). The ability pane
draws the FOCUS curve in its school color over the player's ghosted line — one
shared y-scale, so the ability reads as its share of the player — under a
"Player ▸ Spell" breadcrumb, a stat strip (total, share, hits, crit, average hit,
overkill/overheal, and a SCHOOL card: the game's combo names — Shadowflame, Chaos
— else components joined with +), and the per-target list (school-tinted bars:
name, hits, total, share of the spell). The TUI words the same stats without a
graph. In a comparison, clicking a spell row drills BOTH panes on the pair's one
shared y-scale; a side without the spell says so and keeps its own line;
`Esc`/right-click backs out the ability first, the pair second.

Graph gestures (GUI): LEFT-DRAG on a comparison graph selects a time window —
tables, totals and graphs re-answer for exactly that window
(`Cursor::Compare.range`; a sub-3px wander is a click, not a selection);
RIGHT-CLICK zooms back to the whole fight (captured by the canvas, so it never
falls through and closes the comparison). Item markers wear their ability icon in
a strip along the graph's top edge; HOVERING one highlights every use of that
item on BOTH graphs. The single-player drilldown graph (Damage and Healing views
— each graphs its own metric, worded "dps"/"hps") behaves the same: `g` toggles,
drag zooms client-side (the timeline is whole), right-click zooms out (captured).
A hovered marker's numbers take over the LEGEND row (name, kind × uses, uptime
and window share). Markers with a known `dur_ms` wash their active span under
the curve. On a Σ drilldown the graph underlines its ENCOUNTER LANE — green bars
spanning the visit's boss fights, computed client-side from the segment list
(`ClientState::encounter_spans`: Encounter members of the watched visit,
`start_ms` rebased onto the Overall's — no wire change).

## Daemon (owner: daemon)

One process owns bytes → rows: tail thread, engine (live meter + index + stable
ids + LRU of ≤16 lazily parsed segments), loader worker pool (historical parses
never run on the hub thread), hub (session table, 10 Hz changed-only pushes),
game watcher (3s /proc sweep for a case-insensitive `game_process` substring),
overlay supervisor (spawns/hides/terminates `wowdps-gui --overlay` on game
transitions; a manual hide sticks until the next transition; spawn failures
surface in `Status`). Single instance via a lockfile taken *before* the stale
socket is unlinked. Idle-exit when the last watching session (or overlay child /
exit grace) is gone, unless `--linger`. Config `~/.config/wowdps/config.toml`,
read at startup with a section-aware toml-subset reader: `logs_dir`,
`game_process`, `auto_overlay`, `overlay_exit_grace_secs` (gui keys belong to the
gui, which writes the file with the real `toml` crate). The only persistence is
the index-checkpoint cache in `$XDG_CACHE_HOME/wowdps/index` — never parsed
meters, which is how a cache would become an event store by accident.

## CLI (owner: tui)

Git-style subcommands: `wowdps [--file|--logs]` (TUI client; source conflict with
a running daemon is a hard error naming both), `wowdps gui [--file|--logs]`,
`wowdps daemon [--linger] [--file|--logs]`, `wowdps status`, `wowdps stop`,
`wowdps help`. Any other first word dispatches externally: `wowdps <cmd> [args…]`
execs `wowdps-<cmd>` with the tail verbatim, preferring a sibling of the running
binary (same build) over `$PATH` — `wowdps extract …` runs `wowdps-extract`,
`wowdps mcp` runs `wowdps-mcp`, and the dev shells expose `tools/gen-*.sh` as
`wowdps-gen-<name>` so `wowdps gen-icons` works in a checkout. The retired flag
spellings (`--daemon`, `--gui`, `--stop`, `--status`) error naming the
subcommand, wherever they sit in the argument list. `wowdps-gui [--overlay]`
takes no source flags — it cannot tail. `--overlay` is single-instance: a new
launch evicts the running one (unversioned takeover socket `overlay.sock` beside
the daemon socket, so it works across builds); plain windows may multiply freely.

`wowdps-mcp` (reached as `wowdps mcp`) speaks MCP over stdio: fight tools
`status`, `list_fights`, `fight`, `breakdown`, `compare` are answered from one
lazily connected `ClientKind::Mcp` daemon session — the daemon is spawned only
when a tool first needs it, failures are tool-level errors, and each fight tool
call is answered from the first snapshot matching the cursor it declares (a
client like every frontend); talent tools
`talent_tree`, `decode_talents`, `encode_talents` answer from the per-machine
talent dataset (R14), never the daemon.

## Dependencies

model: zero-dep. core, proto, daemon: stdlib only. mcp: stdlib only (JSON is
hand-rolled like the wire codec — parse never panics). tui: ratatui + crossterm.
gui: iced + iced_layershell + serde/toml. Everything else stdlib unless justified
and signed off. No chrono (hand-parse the timestamp), no tokio (threads +
channels), no serde outside the gui.

## Fixtures (owner: validator)

- `fixtures/sample.txt` — synthetic advanced-format log, 2 encounters (one kill,
  one wipe) + trash, 3 players + 1 pet, covering every modeled event type.
  Expected totals in `fixtures/sample.expected.md` (hand-computed, independent of
  the parser).
- `fixtures/corrupt.txt` — mutated copy, the negative control.
- `fixtures/instance.txt` — R10: keystone visits, suspend/resume, city combat
  between.
- `fixtures/arena.txt` — R13: three skirmishes (win, loss, live-at-EOF) with prep
  healing and a dampening-length lull, gated by `tests/arena.rs`.
