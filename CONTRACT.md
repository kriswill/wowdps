# Module contract (coordinator-owned; changes require coordinator sign-off)

Workspace around a client/server split. Field lists may grow; names, shapes and
rulings below are the agreed interface.

- `wowdps-model` — domain types only (`View`, `Row`, `Class`, `Spec`, `SegmentKind`,
  `SegmentId`, `SegmentInfo`, `ListRow`, `Screen`, `Pane`, `Drill`, `Action`, `fmt`).
  Zero dependencies, no I/O, no parser.
- `wowdps-core` — the engine: `parser`, `meter`, `index`, `tail`. Re-exports model.
  Only the daemon runs it.
- `wowdps-proto` — wire codec (`wire`, `msg`), client library (`client`), client-side
  state machine (`state::ClientState`), plus the shared client-side extras: the
  hand-rolled JSON value (`json`) and the talent dataset + import-string codec
  (`talents`, R14) — hosted here so mcp and gui read the same code. Depends on model
  only — a crate linking proto cannot parse a combat log even by accident.
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
    Version { log_version: u32, advanced: bool,
              build: (u16, u16, u16), project_id: u8 },  // BUILD_VERSION / PROJECT_ID; zeros when absent
    EncounterStart { id: u32, name: String, difficulty: u32, group_size: u32 },
    EncounterEnd   { id: u32, name: String, success: bool },
    CombatantInfo  { guid: String, faction: u32,     // faction = arena SIDE inside a match (R13)
                     talents: Vec<TalentPick>, gear: Vec<GearItem> }, // v19: the line's talent and
                                          // gear brackets, bracket-aware-scanned (empty when absent
                                          // or unbalanced — never a parse failure); spec_id field 25
    Damage { src: Unit, dst: Unit, spell: Option<Spell>, amount: u64, overkill: i64, absorbed: u64, blocked: u64, critical: bool, periodic: bool }, // R17: blocked (partial); ENVIRONMENTAL_DAMAGE carries its envType as a spell named after it (id 0)
    Missed { src: Unit, dst: Unit, spell: Option<Spell>, kind: MissKind, off_hand: bool, prevented: u64 }, // R17: *_MISSED; prevented = BLOCK amount or ABSORB amountMissed, else 0; unknown missType → Other
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
/// The timestamp's timezone offset in minutes east of UTC; None for legacy M/D lines
/// (no year, no offset). `ts_ms` is a LOCAL-time epoch; this turns it into UTC.
pub fn tz_offset_min(line: &str) -> Option<i16>;
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
    pub fn loadout(&self, player_guid: &str) -> Option<&Loadout>; // v19: latest COMBATANT_INFO build
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
    pub encounter: Option<Encounter>, // ENCOUNTER_START id / difficulty / group_size; None off bosses
    pub build: (u16, u16, u16),    // game build from the latest COMBAT_LOG_VERSION (R6 seed) — zeros before one
    pub project_id: u8,            // PROJECT_ID from the same line (1 = retail)
    pub log_version: u32,          // log format version from the same line
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
    /// v19: the player's COMBATANT_INFO loadout as known to THIS segment —
    /// the latest line at or before it, NEVER a later segment's (warm and
    /// lazy answers must agree). Follows the classes/specs lifecycle:
    /// authoritative, seeded into later segments, latest wins PER FIELD —
    /// a bracket that parsed empty (absent or truncated mid-write) carries
    /// no information and never wipes that field's established data.
    pub fn loadout(&self, player_guid: &str) -> Option<&Loadout>;
    /// R16: the lowest hostile-NPC health fraction observed while this
    /// Encounter was open, as a whole percent rounded down (0 on a kill);
    /// None off raid bosses (Trash, arena, Overall) or without a report.
    pub fn best_pct(&self) -> Option<u16>;
    /// R17: one player's mitigation split (pets folded onto owners at read time,
    /// like `rows`); `None` when nothing was ever swung at them.
    pub fn mitigation(&self, player_guid: &str) -> Option<Mitigation>;
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

pub enum View { Damage, Healing, Interrupts, CrowdControl, Dispels, Deaths, Taken }  // Taken: R17, a rate view (DTPS)
pub struct Row {
    pub key: String,               // player guid (meter) / spell or target name (breakdown)
    pub label: String,             // display name
    pub amount: u64,               // damage done, healing done, or event count
    pub extra: u64,                // overheal for Healing; overkill for Damage; absorbed for Taken (R17); else 0
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

Each ruling is two rows: **the call** — the decision at a glance, and what a
conflicting design change would collide with — then its full binding detail,
right beneath it.

<table>
<thead><tr><th>R#</th><th>Ruling</th><th>The call</th><th>A conflicting change would…</th></tr></thead>
<tbody>
<tr><td>R1</td><td>Damage</td><td>Row amount = event <code>amount + absorbed</code>; extra = overkill (≥0). SWING_DAMAGE counted, its LANDED twin / <code>*_SUPPORT</code> → Other; DAMAGE_SPLIT excluded.</td><td>Move every fixture damage total; break the "absorbed damage is damage done" convention shared with in-game meters.</td></tr>
<tr><td></td><td colspan="3">Row amount = per-event <code>amount + absorbed-field</code> (absorbed-by-shield damage counts as damage done, meter convention); extra = overkill clamped ≥ 0. Count SWING_DAMAGE only (SWING_DAMAGE_LANDED → Other); <code>*_SUPPORT</code> → Other; DAMAGE_SPLIT excluded from offensive totals.</td></tr>
<tr><td>R2</td><td>Healing</td><td>Amount = effective (amount − overheal); extra = overheal. SPELL_ABSORBED credits the ABSORBER; stagger/cheat-death self-absorbs excluded.</td><td>Move healing totals; double-count absorbs against R3.</td></tr>
<tr><td></td><td colspan="3">Row amount = effective healing (amount − overheal); extra = overheal. SPELL_ABSORBED credits the ABSORBER with healing (no overheal component). The damage-event absorbed field never contributes to any healing number. Stagger / cheat-death self-absorbs (114556, 31850, 31230, 115069) excluded from healing.</td></tr>
<tr><td>R3</td><td>Absorb attribution</td><td>One source per direction: SPELL_ABSORBED → healing, the damage-event absorbed field → damage.</td><td>Double-count absorbs in one view or drop them from the other.</td></tr>
<tr><td></td><td colspan="3">SPELL_ABSORBED is the sole source for absorb-as-healing; the damage-event absorbed field is the sole source for absorb-as-damage. Different views, different actors — no double count.</td></tr>
<tr><td>R4</td><td>Segment boundaries</td><td>ENCOUNTER_START opens (closing any open), END closes exactly (no grace window); other combat accrues to Trash, split after >60s quiet; every ZONE_CHANGE closes open Trash.</td><td>Change which events land in which segment — the index scanner mirrors these rules and must stay in lockstep.</td></tr>
<tr><td></td><td colspan="3">ENCOUNTER_START opens an Encounter segment (closing any open one); ENCOUNTER_END closes it exactly (known ~1–3% DoT-tail divergence vs Warcraft Logs; accepted, no grace window). Combat outside encounters accrues to a Trash segment; a new Trash segment starts after >60s with no combat events. Every ZONE_CHANGE also closes the open Trash segment — a teleport is a hard location break; without it pre-instance trash would bleed into (and out of) a visit (R10).</td></tr>
<tr><td>R5</td><td>Pets</td><td>Summoned units fold onto their owner — never separate meter rows; by-spell rows aggregate per pet NAME.</td><td>Bury drilldowns under swarm-pet instances, or split owner totals.</td></tr>
<tr><td></td><td colspan="3">Damage/heals by a unit summoned by a player (SPELL_SUMMON or advanced-field ownerGUID) count toward the owner — never separate meter rows. Breakdown by-spell label: "{spell} ({petName})", aggregated by pet NAME — swarm specs summon dozens of same-named instances per fight; one row per name, and differently named pets stay separate.</td></tr>
<tr><td>R6</td><td>Version seam</td><td>Mid-log COMBAT_LOG_VERSION: close the segment, reset the pet map, SUSPEND the visit (the re-fired ZONE_CHANGE resumes it).</td><td>Split keystone runs on /reload, or leak pet ownership across seams.</td></tr>
<tr><td></td><td colspan="3">Mid-log COMBAT_LOG_VERSION is a hard boundary: close the open segment, reset the pet-owner map, SUSPEND the open visit (R10) — a mid-run /reload writes a version line with the key still in progress, and the ZONE_CHANGE the game re-fires right after resumes the visit; a seam elsewhere closes it at the next ZONE_CHANGE as usual.</td></tr>
<tr><td>R7</td><td>Duration</td><td>Encounters = START..END; Trash = first..last combat event.</td><td>Deflate trash DPS with idle time, or unclock encounters.</td></tr>
<tr><td></td><td colspan="3">Encounter segments = ENCOUNTER_START..ENCOUNTER_END exactly. Trash segments = FIRST..LAST combat event inside the segment (active combat time, like in-game meters) — never open..close, which counts idle time and deflates DPS.</td></tr>
<tr><td>R8</td><td>Class/spec inference</td><td>Inferred from a FIXED list of player-sourced spell events against the generated <code>class_spells</code> table; segment-local; COMBATANT_INFO overwrites and alone persists; never opens a segment.</td><td>Widening sources moves fixture expectations; carrying inference forward breaks lazy/full parity.</td></tr>
<tr><td></td><td colspan="3">Outside instances COMBATANT_INFO never fires, so class (and, when the spell is unique to one specialization, spec) is inferred from player-sourced spell events — Damage/Heal/Interrupt/Dispel/AuraApplied via <code>src</code>, SPELL_ABSORBED via the absorbing shield's caster — against the generated table <code>core/src/class_spells.rs</code> (spell id → class/spec; <code>tools/gen-class-spells.sh</code> from the local install's DB2s: class skill lines + SpecializationSpells + trait trees; multi-class spells excluded, class-wide spells carry no spec). Inference is SEGMENT-LOCAL: it writes only the open segment, never the carried-forward maps, so lazy loading reproduces it exactly. COMBATANT_INFO is authoritative — it overwrites inference and is the only class/spec source that persists across segments. Inference never opens or extends a segment (scanner lockstep). The source list is FIXED — widening it (e.g. with <code>Cast</code>) would move fixture expectations. v19: the line's talent and gear brackets ride the same event into per-player <code>Loadout</code>s with the same lifecycle — carried forward via segment seeding, latest wins PER FIELD (a bracket that parsed empty, whether absent or truncated mid-write, carries no information and never wipes that field's established data) — so lazy loads reproduce them exactly (COMBATANT_INFO lines are already seed lines).</td></tr>
<tr><td>R9</td><td>Deaths & recap</td><td>First-death order; per-player 32-event ring (damage w/o absorbed part + gains), drained at UNIT_DIED, latest death wins; recap newest-first with hp/gain; never opens a segment.</td><td>Break recap parity between lazy and full replay, or reorder the Deaths view.</td></tr>
<tr><td></td><td colspan="3"><code>rows(Deaths)</code> lists players in FIRST-death order, amount = 1 per death. Each Segment keeps a bounded per-player ring (<code>RECAP_CAP = 32</code>) of recent events on that player: damage hits (amount = the per-event <code>amount</code> alone — the absorbed part never touched their health and appears as its own gain entry) and gains (heals at effective value with overheal in extra; consumed absorbs via SPELL_ABSORBED). UNIT_DIED drains the ring into that player's recap — latest death wins — so <code>breakdown(guid, Deaths)</code> returns (timeline newest-first, killing blow with its overkill-in-extra leading; attacker totals sorted desc, gains excluded, source-less damage bucketed under its spell name). Timeline rows carry <code>hp</code>/<code>gain</code>; labels "{spell} ({source})", spell alone when the source unit is nil. HP comes from the line's own advanced block when it describes the victim, else back-fills onto the newest HP-less entry from the next advanced line describing them within 1s (SWING_DAMAGE → its LANDED twin; SPELL_ABSORBED → the paired damage line). Health reports and recap bookkeeping never open or extend a segment; the ring is segment-local (lazy-load parity).</td></tr>
<tr><td>R10</td><td>Visits & Overall</td><td>Difficulty ≠ 0 zoning opens a visit (suspend/resume on zoning; keyed visits resume on map alone); CHALLENGE_MODE_START resets the visit; Σ = members merged, duration = Σ of member durations — except a keyed Σ runs on the KEY clock and reports the TIMED verdict vs par.</td><td>Split or merge runs wrongly, mis-time keys, or break index/lazy/replay agreement on ordinals.</td></tr>
<tr><td></td><td colspan="3">A ZONE_CHANGE with difficulty ≠ 0 opens a <em>visit</em> (map_id + difficulty + zone name); ordinals index the file's visit table in order. Zoning out (difficulty 0) SUSPENDS it — segments recorded outside carry no visit — re-entering the same (map_id, difficulty) resumes it; a different instance closes it. A KEYED visit resumes on map_id alone: mid-run re-fires (reloads, reconnects) carry the keystone difficulty instead of the one stamped at the door, and a split would orphan the END onto an unkeyed visit.<br>• Every CHALLENGE_MODE_START on the current visit's map is a visit boundary — the dungeon resets and the key's clock starts with the countdown, not at the door: the visit (and any open trash) closes and a fresh KEYED visit opens, so pre-key activity never joins the run's Overall.<br>• CHALLENGE_MODE_END counts only for a keyed visit (the zeroed reset fired on entry, before any START, is ignored); it sets <code>completed</code> from its success flag and <code>official_ms</code> from its totalMs — the game's own run time, death penalties included. The success flag only means "completed" (1 even in overtime), so a keyed visit's REPORTED outcome (<code>Visit::verdict</code>, shown as segment <code>success</code>) is the TIMED verdict against the dungeon's par (generated <code>keystone_timers.rs</code>, MapChallengeMode keyed by START's challengeID): <code>official_ms &le; par</code> once the END fired; before it, a run already past par reports failed — OVER shows the moment the timer elapses (up to 15s per death late — live clocks carry no death penalties); an abandoned keyed run (END success 0) is failed; unknown challengeID falls back to the END flag.<br>• Segments opened while zoned in carry the visit's ordinal. The visit's OVERALL (<code>Meter::overall</code>) is a synthetic <code>SegmentKind::Overall</code> segment: members' counters merged (tallies sum; identity maps union, later member wins; death order first-occurrence across members; each player's latest recap wins), duration = SUM of member durations (R7 per member, an open member cut at its last combat event), success = <code>completed</code>, name = <code>Visit::display_name()</code>.<br>• EXCEPT: a KEYED visit's Overall clock is the key timer, not combat time — <code>official_ms</code> once the END fired (exact, frozen), else wall clock from <code>start_ms + KEY_COUNTDOWN_MS</code> (10s — the in-game timer starts when the activation countdown ends) to <code>end_ms</code>/now, clamped ≥ 0. Live estimates lag the in-game timer by 15s per death until the END corrects them. A keyed Σ row's per_sec is over this key clock — run DPS, not combat DPS.<br>• Live and lazy paths both build the Overall by merging members, so index-then-lazy equals full replay by construction. A scan cut mid-visit splits the visit at <code>live_offset</code>: the <code>open_visit</code> prefix carries only members closed before it — bytes and clock — and the open member belongs exclusively to the live tail (which replays it from its first line), so merging prefix + live counts every member exactly once.<br>• ZONE_CHANGE / CHALLENGE_MODE_* lines are SEED lines: replaying them ahead of any slice (or the live tail) reconstructs the visit table with file-consistent ordinals everywhere. In the combined segment list the Overall row precedes its visit's first member, and exists only once the visit has a member.</td></tr>
<tr><td>R11</td><td>Meaningful segments</td><td>A closed segment earns a list row only if: Encounter, non-empty enemy tally, player-damaged-player, or a player died. Live always surfaces. Hidden segments still exist internally (ids positional, parity over ALL).</td><td>Flood the list with NPC noise, or — worse — shift segment ids by actually deleting.</td></tr>
<tr><td></td><td colspan="3">The log records the whole neighborhood, so world Trash can be pure NPC-vs-NPC noise or out-of-combat heals. A segment is WORTH A LIST ROW (<code>Segment::counts</code>, mirrored by the scanner into <code>SegmentMeta::counts</code>) iff it is an Encounter, its enemy tally is non-empty (a friendly damage event landed on a hostile — the same tally that names pulls), a player damaged ANOTHER player (duels, world PvP; self-damage excluded), or a player died in it (the recap must survive). A live segment always surfaces; one that closes without counting is dropped from the daemon's list (live and indexed paths alike) but still exists internally: ids stay positional, parity is over ALL segments, and Σ overalls merge every member regardless. A Σ row is listed only in front of a visible member — a fully filtered visit leaves no dangling Σ-only block.</td></tr>
<tr><td>R12</td><td>Timelines & markers</td><td>Per-guid 1s damage/heal grids + bounded curated item markers (class_spells vetoes the generous item table); all segment-local, never opening segments; Σ merges curves on the visit clock. Comparison pairing is client state.</td><td>Desync graphs from totals, mark class spells as trinkets, or break marker parity on lazy loads.</td></tr>
<tr><td></td><td colspan="3">Every segment keeps, per acting guid, damage bucketed on a fixed 1s grid anchored at <code>start_ms</code> (<code>Segment::timeline</code>; pets fold onto owners exactly like <code>rows</code>/<code>breakdown</code>; bounded by <code>MAX_BUCKETS</code> so a corrupt clock costs a clamp, not an allocation), and effective healing (R2 amounts) on the same grid in its own series (<code>heal_timeline</code>, same folding, same markers).<br>• Per PLAYER guid, a bounded list (<code>MARK_CAP = 256</code>) of ITEM MARKERS. A marker's spell is classified by the generated <code>core/src/item_spells.rs</code> (spell id → <code>ItemKind</code>; <code>tools/gen-item-spells.sh</code> from Item / ItemEffect / ItemXItemEffect, with <code>SpellEffect.EffectTriggerSpell</code> chased two levels out of trinket effects so proc buffs — never the item's own listed spell — are covered). <code>class_spells</code> WINS that lookup: the chase is generous and also claims ordinary class spells, which must never draw an item marker.<br>• A <code>Cast</code> (SPELL_CAST_SUCCESS) by a player marks <code>TrinketUse</code> for a trinket spell, <code>Consumable</code> for anything else. A Buff <code>AuraApplied</code> on a player marks <code>TrinketProc</code> — trinkets only, and only when no cast of that spell by that player precedes it within 2s (an on-use trinket's own buff is its use); the same proc re-applying within 500ms is one proc (buffs refresh as they stack).<br>• SPANS: a Buff <code>AuraRemoved</code> on a player closes the newest still-open mark of that spell, setting <code>dur_ms</code> (unknown stays 0, draws no span); a Buff re-applying while a mark of that spell is OPEN is a refresh, not a new mark.<br>• EXTERNALS: spells in the CURATED <code>EXTERNAL_BUFFS</code> list (the Bloodlust family + Power Infusion) mark <code>External</code> when the buff LANDS on a player — checked before the class-spells veto (which would otherwise eat Power Infusion); the list is hand-picked so persistent raid buffs can never clutter a graph.<br>• Casts and aura bookkeeping NEVER open or extend a segment (scanner lockstep, like R8/R9); marker state is segment-local, so lazy loading reproduces timelines and markers exactly. <code>Cast</code> is deliberately NOT an R8 source.<br>• Buckets and markers merge on <code>absorb</code> (R10): member curves shift by <code>(other.start_ms − self.start_ms) / bucket_ms</code>, so a visit's Overall spans the visit's wall clock. Markers are stored absolute, rebased by <code>timeline()</code>.<br>• The comparison itself is a CLIENT concern: <code>ClientState</code> holds at most two picked players, a third pick replaces the older, and <code>Screen::Compare</code> is reachable only with BOTH picked — a half-made pair keeps the meter up. Segment navigation (<code>[</code>/<code>]</code>, list jumps, return-to-live) never breaks an open comparison: the pair sticks and the new segment's sides are requested; only Back/right-click (or unpicking) closes it. Graph mode (rolling DPS / cumulative) is purely local — both curves come from buckets in hand.</td></tr>
<tr><td>R13</td><td>Arena</td><td>ARENA_MATCH_START..END is an Encounter named from the last zone; verdict = home side (from match-local COMBATANT_INFO factions) vs winningTeam, worded WIN/LOSS; the post-END tail is unlisted <code>noise</code>; <code>enemy</code> rows split teams in arena segments only.</td><td>Turn matches back into anonymous trash, flip verdicts, or let the noise tail steal the live meter.</td></tr>
<tr><td></td><td colspan="3">Arenas zone in with ZONE_CHANGE difficulty 0, so R10 never sees them; without this ruling a match records as anonymous Trash. ARENA_MATCH_START (mapID, matchType) opens an <code>Encounter</code>-kind segment — closing whatever was open, exactly like ENCOUNTER_START — named <code>"{zone} ({matchType})"</code> from the LAST ZONE_CHANGE's name at ANY difficulty (<code>Meter::last_zone</code>, mirrored by the scanner and persisted in <code>ScanState::last_zone</code> so a checkpoint resume between zone-in and gates still names the match; a log begun mid-match falls back to "Arena").<br>• VERDICT: ARENA_MATCH_START's trailing teamID is a dead constant 0 (verified live), so the HOME side comes from the match's own COMBATANT_INFO lines — field 2 ("faction") is the player's arena side, re-fired right after the START. Factions are MATCH-LOCAL state; the home side resolves at the first friendly-flagged (reaction 0x10) player source of a damage event (every friendly shares one side, so resolution order cannot change the answer — which lets meter and scanner stay in lockstep without identical iteration order). ARENA_MATCH_END closes the segment with <code>success = (winningTeam == home)</code> — verdict-less if the home side never resolved — so kill/wipe colors read as win/loss with no extra wire fields.<br>• Encounter kind buys the rest: R7 clocks the match START..END (dampening lulls longer than the trash gap cannot split it), R11 always counts it, and gate-prep activity before the START stays behind in (non-counting) Trash.<br>• All arena state is match-local, held only while the match's segment is open: a stray END with no START closes nothing; a mid-match COMBAT_LOG_VERSION seam (R6) drops it, orphaning the match's END — which also keeps it out of checkpoints. Solo Shuffle logs one START/END pair around all six rounds; rounds are not split (future work).<br>• THE TAIL IS NOISE: pets and DoTs keep hitting between ARENA_MATCH_END and the teleport out; that decided-arena combat opens a Trash segment flagged <code>noise</code> — it exists internally (ids positional, parity over ALL segments) but NEVER earns a list row, not even live (R11's live exception does not apply), never announces a <code>SegmentOpened</code>, and the daemon's Live cursor skips it, so the meter stays parked on the finished match and its verdict. The window (<code>arena_over</code>) opens at any ARENA_MATCH_END — unconditionally, an END whose START predates the log still leaves us in a decided arena — and closes at any ZONE_CHANGE, the next ARENA_MATCH_START, or a version seam. It spans a region with no open segment, so it travels in <code>ScanState</code>; ARENA_MATCH_END lines are SEED lines so a lazy load of the tail reproduces the flag.<br>• TEAMS: enemy players earn meter rows like anyone else (<code>Player-</code> GUIDs), so every meter row carries <code>enemy</code> — the unit-flags reaction bit (0x40 Hostile), set ONLY in <code>arena</code> segments (hostile-flagged players in the open world — war mode, duels — never split the chart), segment-local like names/flags so lazy loads agree. Sorted views order rows (enemy, amount desc, label): the friendly team leads, the enemy team trails as one block, and a renderer splits the chart at the first <code>enemy</code> row (GUI surfaces draw a divider; the TUI reads enemy names in red; Deaths keeps pure death order, no divider). Breakdown rows are never <code>enemy</code>. Match segments carry <code>arena = true</code> (Segment, SegmentMeta, SegmentInfo, ListRow alike), and every surface words their <code>success</code> as the HOME TEAM'S outcome — WIN/LOSS, never KILL/WIPE.<br>• Gated by <code>fixtures/arena.txt</code> + <code>tests/arena.rs</code> (replay semantics, scanner parity, lazy-load parity, checkpoint resumption).</td></tr>
<tr><td>R16</td><td>Boss health</td><td>Inside an open raid-boss Encounter, the advanced block's health report for a hostile NPC (Creature-/Vehicle- guid) is a boss-health observation, tracked per NPC; the BOSS is the NPC with the largest max health seen (any NPC with at least half that is a boss too — councils — provided its creature id spawned once: an add pack is never a council), and <code>Segment::best_pct</code> = the lowest fraction among the bosses still standing, whole percent rounded down — a member that is down (under 0.1 %: the game parks a boss it will not let die yet at 1 HP) is progress, not the grade; 0 when every boss is down, and always 0 on a kill (ENCOUNTER_END success — a scripted death lands no 0/max report); never 0 for an add or a friendly guardian dying. Segment-local; never Trash, arena or Overall; never opens or extends a segment.</td><td>Grade progression on the wrong number, or break lazy/full parity by carrying observations across segments.</td></tr>
<tr><td>R17</td><td>Damage taken &amp; mitigation</td><td>Every damage event whose destination is a player or pet (pets fold onto owners, R4) is recorded a second time on the DESTINATION as <code>View::Taken</code>: amount = R1's <code>amount + absorbed</code> (the log's amount is post-block, so blocked is NOT added — per segment Σ every actor's Damage by_target over friendly names = Σ Taken rows + Σ <code>stagger_ticked</code>, exactly), extra = absorbed, by-spell = taken by ability, by-target = taken by ATTACKER NAME (R5's pet-by-name precedent; never a guid). A <code>*_MISSED</code> line has no damage twin: it is count 1 / amount 0 on the Taken row and its drill rows (a player who was only dodged still earns a row — Taken lists on count &gt; 0), and its BLOCK amount or ABSORB amountMissed is PREVENTED damage in the per-player <code>Mitigation</code> record (partial absorbs + blocks + full absorbs + blocks = <code>mitigated</code>; <code>mitigated_pct</code> = mitigated / (taken + full-miss amounts); dodge/parry/miss carry no amount and are counts only). SPELL_ABSORBED is never read by Taken (R3). STAGGER: the <code>NON_HEALING_ABSORBS</code> amount consumed on a player is <code>stagger</code>, a subset of absorbed (R3's premise), never added again; the self-sourced Stagger ticks (124255, src = dst) that re-deal it are EXCLUDED from Taken and tallied as <code>stagger_ticked</code> (their amount + absorbed, exactly what Taken would have recorded — so the identity above is exact; R1 still counts a tick as damage done). ENVIRONMENTAL_DAMAGE is labeled by its envType; a nil source is named "Environment". The mitigation map is keyed by raw destination guid and folds at read time. Segment-local; never opens, extends or splits a segment (the scanner ignores *_MISSED and the NON_HEALING_ABSORBS SPELL_ABSORBED; the meter records either only into an already-open segment that is NOT past the R4 trash gap — it mirrors <code>ensure_combat</code>'s split predicate without acting on it — and never touches last_ms). Consequently a miss logged more than 60 s after a Trash segment's last combat lands in no segment, and a stagger absorb logged before the pull's first hit (the game logs SPELL_ABSORBED just before the hit it shields: after an ENCOUNTER_END, a lull, or at the log's start) is NOT attributed — the pull's byte range starts at the hit, so a lazy load could never see that line, and full replay must agree with it.</td><td>Break the taken = dealt identity, count a staggered hit twice, or open a segment from a miss and break scanner/meter lockstep.</td></tr>
<tr><td></td><td colspan="3">The report comes from any line whose advanced block describes a hostile NPC — a <code>Creature-</code>/<code>Vehicle-</code> guid whose unit flags carry the hostile reaction bit (0x40; a friendly guardian's totem is a Creature too) — its own attacks, and the <code>_LANDED</code> twins of hits on it (which is how the killing blow lands a <code>0/max</code> report). Reports are kept PER NPC (lowest fraction, largest max); at read time the NPC with the largest max health is the boss, every NPC with at least half of it is a boss too (council fights) as long as its creature id — the sixth field of the guid — spawned exactly once (eighteen Manifestations of Dread at 223M each are an add pack, not a council; if the strict set is empty because a boss re-spawned under a new guid, every NPC big enough counts). The answer is the lowest fraction among the bosses NOT DOWN, where down is under 0.1 % — the game holds a boss it will not let die yet at 1 HP (First Mate Nama, Zul'jan), which is 0 % to any raid leader — so a council member that fell is progress made and the number on a wipe is how close the LAST one got; every boss down is the kill, 0. So an add or an enemy pet dying while the boss stands at 70% grades the pull 70, and a twin dying while its sibling stands at 60% grades it 60. Fractions compare exactly (cross-multiplied); the percent is <code>floor(current × 100 / max)</code>. The history store writes it to the fight card as <code>best_pct</code>, and <code>Progression</code> answers carry each night's lowest — "best-percent progression" (spec-history-store §4).</td></tr>
<tr><td>R14</td><td>Talent dataset & codec</td><td><code>gen-talent-trees</code> builds a per-machine talents.json from the install's Trait DB2s; the mcp codec speaks import-string v2 from the dataset alone (no daemon); gated on a real string's byte-identical round-trip.</td><td>Commit Blizzard-derived data, or drift the codec from the game's real serialization.</td></tr>
<tr><td></td><td colspan="3"><code>tools/gen-talent-trees.sh</code> joins the install's Trait DB2 tables into <code>$XDG_DATA_HOME/wowdps/talents.json</code> — a per-machine cache like the icon bins (Blizzard-derived strings never enter the repo), deterministic per build. The ACTIVE tree per class comes from the class SkillLine (matched by display name, CategoryID 7) → SkillLineXTraitTree; TraitTreeLoadout alone also names retired and dev/test trees. Each tree's <code>nodeOrder</code> is every node id ascending — exactly the walk order of the in-game import string (serialization version 2: 6-bit LSB-first groups over the base64 alphabet; header 8-bit version, 16-bit spec, 128-bit tree hash, zero = skip validation; per node selected(1) / purchased(1) / partially-ranked(1)+ranks(6) / choice(1)+entry-index(2); granted nodes stop after the purchased bit; the choice bit follows the node TYPE — Selection/SubTreeSelection — not the entry count). The codec lives in <code>proto::talents</code> (stdlib file IO, dataset alone, no daemon round-trip) with the JSON value type beside it in <code>proto::json</code>; the mcp tools and the gui's talent viewer both speak through it (mcp re-exports the modules). Encode zero-fills the hash; a missing dataset is a caller-level error naming the generator. Gate: byte-identical decode→encode round-trip of a real exported string — the env-gated test <code>real_talent_string_round_trips_byte_identically</code> (crates/mcp/tests/server.rs; <code>WOWDPS_REAL_TALENT_STRING=C… cargo test -p wowdps-mcp -- --ignored real_talent</code>), run per patch alongside dataset regeneration; committed tests cover the same round-trip on a synthetic fixture.</td></tr>
<tr><td>R15</td><td>Count views & labels</td><td>Interrupts drill: "{kicked} ({ability})"; CC drill: "{cc} ({victim})"; CC view counts a curated loss-of-control list (exactness not gated).</td><td>Answer the wrong question in the drill panes ("what got kicked" / "who got locked down").</td></tr>
<tr><td></td><td colspan="3">The Interrupts by-spell pane answers "what got kicked" — "{interrupted spell} ({interrupt ability})"; the CrowdControl pane answers "who got locked down" — "{cc spell} ({victim})". Meter-row counts are unchanged by these labels. The CrowdControl view counts AuraApplied debuffs whose spell is in a small built-in loss-of-control list (stuns, roots, incaps, fears — <code>const CC_SPELLS</code>/heuristic; exactness not gated).</td></tr>
</tbody>
</table>

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
    pub encounter: Option<Encounter>, // mirror of Segment::encounter
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

## Wire protocol (owner: proto) — `PROTO_VERSION = 22`

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
| `GetLoadout` (v19: one player's COMBATANT_INFO loadout for one segment) | 0x07 | `SetVisible` | 0x87 |
| `GetHistory` (v20: one of the history store's fixed questions — `Fights` with filters/sort/limit + `after_id` paging (answer carries `total`; v22: + trailing `role`, the SUBJECT's role — `guid`, else the owner — a no-op without a subject), `Progression` per boss+difficulty, `Trend` per player+spec (v22: by `measure` — dps / hps / dtps / mitigated_pct — in place of the v20 view) — always answered, empty when the store is disabled) | 0x08 | `Fatal` | 0x88 |
| `GetFight` (v20: one stored fight — card + the view's rows, + the drilled player's breakdown from the details tier / death recap; trailing `boss` names one of a key's member bosses — listed on the card as `bosses` — parsed from the log on demand and answered with its own rows, nothing stored) | 0x09 | `CompareSnapshot` | 0x89 |
| `PinFight` (v20: protect / release a stored fight from retention) | 0x0A | `Loadout` | 0x8A |
| `ImportLog` (v20: queue an import sweep of a log or directory — `wowdps history import`) | 0x0B | `History` (answers GetHistory / PinFight / ImportLog / Regrade) | 0x8B |
| `Regrade` (v20: rewrite stored cards from their logs — one fight id, or a boss + difficulty — pins and annotations kept; answered `Regraded { queued }`, the rewrites ride the import queue — `wowdps history regrade`) | 0x0C | | |
| | | `Fight` (answers GetFight; `None` = unknown or evicted) | 0x8C |
| | | `HistoryChanged` (unsolicited, every session, per stored / pinned fight) | 0x8D |

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

`GetLoadout { req_id, SegmentRef, guid }` is a one-shot like `GetStatus`,
answered with `Loadout { req_id, guid (echoed), Option<Loadout> }` with control
semantics (ordered, never dropped). The daemon resolves the `SegmentRef` exactly
as a `Watch` would; a segment not yet resident is loaded and the reply DEFERRED
until the loader delivers — never answered with a placeholder — and a one-shot
is ALWAYS answered: an unknown guid, a player whose COMBATANT_INFO never fired,
a rotated/unknown id, or a failed load all answer `loadout: None`, never an
error. `Live` answers from the live meter, whose loadout map has seen every
seed line since log start. The payload (`Loadout { spec_id: u16 raw specID
(0 = none), talents: Vec<TalentPick { node_id, entry_id, rank: u32 }>, gear:
Vec<GearItem { item_id, ilvl: u32, enchants/bonus_ids/gems: Vec<u32> }> }`) is
raw log data only — resolving picks against the talent dataset stays client-side
in `proto::talents` (R14; `picks_to_selections` is the conversion into the
codec's selections).

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
| 19 | ClientMsg + `GetLoadout 0x07` (req_id, SegmentRef, guid); DaemonMsg + `Loadout 0x8A` (req_id, guid, Option<Loadout>); payload structs `Loadout`/`TalentPick`/`GearItem` |
| 20 | `SegmentList` + trailing Option<u64> `log_id` (the tailed log's identity once its header is whole; with a closed row's `start_ms` — plus an `s` mark for an Overall row — it names the history-store fight id); `SegmentInfo`/`ListRow` + Option<Encounter> `encounter` (ENCOUNTER_START id, difficulty, group_size — the history store's key; None off bosses); `Status` + `HistoryStatus` (bool enabled, u32 fights, u32 dropped, u32 importing, bool owner_inferred, Option<String> error); ClientMsg + `GetHistory 0x08` (req_id, HistoryQuery: u8 code — 0 Fights / 1 Progression / 2 Trend — then that variant's fields), `GetFight 0x09` (req_id, fight_id, view, Option<drill>), `PinFight 0x0A` (req_id, fight_id, bool), `ImportLog 0x0B` (req_id, path); DaemonMsg + `History 0x8B` (req_id, HistoryAnswer: u8 code — 0 Fights / 1 Progression / 2 Trend / 3 Pinned / 4 Imported), `Fight 0x8C` (req_id, Option<StoredFight> = FightCard + Vec<Row> + Option<Breakdown>), `HistoryChanged 0x8D` (fight_id); payload structs `FightCard`/`CardPlayer`/`KeyInfo` (hashes as u64, tz_min as u16 bits, spec as raw id), `Night`, `TrendPoint`. |
| 21 | R17: `View` + `Taken` (code 6; the socket is renamed `wowdps-v21.sock` by the bump); `Breakdown` + trailing Option<Mitigation> `mitigation` — embedded inside `Snapshot` / `StoredFight`, so its presence byte is ALWAYS written (`None` = `00`), never a frame-trailing omission; the record is a fixed 88 bytes: six u64 in declaration order (`absorbed`, `blocked`, `absorbed_full`, `blocked_full`, `stagger`, `stagger_ticked` — overkill is the R9 recap's, per death, not a mitigation field) then the ten miss counts as u32 in `MissKind::ALL` order (Dodge, Parry, Block, Miss, Absorb, Immune, Deflect, Evade, Reflect, Resist); present iff the drilled view is Taken. Store consequence (record, not wire): `FightRows.views` gains the `taken` key via `VIEW_KEYS`, so the rows tier carries Taken rows. |
| 22 | R17 step 2b (the socket is renamed `wowdps-v22.sock`): `CardPlayer` + trailing u64 `taken`, u64 `mitigated`, u64 `prevented` (= absorbed_full + blocked_full), f64 `dtps` (32 bytes after `deaths`; `mitigated_pct` is DERIVED — `CardPlayer::mitigated_pct()` = mitigated / (taken + prevented) × 100 through the model's one `mitigated_pct` helper, shared with the live `Mitigation` record — and never travels); `HistoryQuery::Trend`: the u8 `view` (a View code) is REPLACED by u8 `measure` — `TrendMeasure` 0 Dps / 1 Hps / 2 Dtps / 3 MitigatedPct — in the same position (v21 pinned no Trend bytes; v22 does). `TrendPoint.amount` is the measure's numerator (damage / healing / taken / mitigated) and `per_sec` its value (a rate, or for MitigatedPct the percentage); a `Day` / `Week` bucket folds `per_sec` as a running MEAN of the per-fight values — a mean of pcts, exactly as Dps-by-day is already a mean of rates — never amount / duration. `HistoryQuery::Fights` + trailing Option<Role> `role` (u8: Tank 0 / Healer 1 / Dps 2, anything else `BadTag`) = the SUBJECT's role (`guid`, else the owner) by their spec on the card; with no subject the filter is a no-op. Store consequences (record, not wire): the card writes `taken` / `mitigated` / `prevented` / `dtps` + the derived `mitigated_pct` per player (a PR #16 card reads 0 / 0.0 and derives 0); `rows/<id>.json` + `mitigation`: per player `{ guid, record: Mitigation as an object with `misses` keyed by `MissKind::name()` (all ten written), taken_spells (top `TAKEN_SPELLS_CAP` = 16 by amount), other: TakenOther { amount, extra, count, n abilities folded }, taken_sources (by attacker name, uncapped) }` on EVERY fight — rows-only, the details tier holds no copy; absent on an older file = empty. |

## Client state & behavior (owner: proto; keybinds owner: clients)

`state::ClientState` holds screen/view/selection/drill, the cached last snapshot,
and the comparison pair + graph mode; `apply(Action)`/`on_msg(DaemonMsg)` return
the `ClientMsg`s to send. Held-key `Up`/`Down` clamps against the cache and never
round-trips.

Keybinds: list — `j/k`/arrows move, `Enter` opens, `q` quit. Meter — `d/h/i/c/x/K/T` (T = Taken, R17)
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
`game_process`, `auto_overlay`, `overlay_exit_grace_secs`, and the history
store's flat `history_enabled` / `history_dir` / `history_store_trash` /
`history_keep_per_encounter` / `history_keep_details_per_encounter` /
`history_characters` (one comma-separated string — the reader has no list type).
Gui keys belong to the gui, which writes the file with the real `toml` crate and
carries every key it does not own through a save untouched.

Persistence is exactly two things. The index-checkpoint cache in
`$XDG_CACHE_HOME/wowdps/index` — never parsed meters, which is how a cache
would become an event store by accident. And the **history store** (roadmap
item 1, `docs/spec-history-store.md`): per-fight JSON documents under
`$XDG_DATA_HOME/wowdps/history/v1/` (`fights/<id>.json` card — per player the
damage / healing totals and rates, deaths, and since v22 the R17 tank measures
`taken` / `mitigated` / `prevented` / `dtps` with `mitigated_pct` derived on
write, never read back; `rows/<id>.json` the seven views' rows + death recaps
+ (v22) every player's mitigation record with both Taken drills, the by-ability
list capped at 16 with the rest rolled into `other`; `details/<id>.json` breakdowns + timelines
for kills / bests / pins, `loadouts/<hash>.json` content-addressed,
`annotations/<id>.ndjson` reserved), each written `.tmp` + rename through
`proto::history`. The boundary: a stored record is a *derivable fight summary*
— nothing `Meter` cannot recompute from the log, nothing keyed per event, and
the files are the truth (the in-memory index is rebuilt from the cards on
start; any file may be deleted). A fight is stored when it closes on the live
meter after `CaughtUp` (one `Segment` clone on the hub thread, then a bounded
`try_send` — a full queue drops and counts, never stalls the meter), and by
import for everything closed before that (the tailed log's index, plus a
start-up sweep of every log in `logs_dir`, one loader job outstanding at a
time). Fight identity is `fnv64(first complete line)-start_ms`, so restarts,
rescans and replays write nothing twice; a record still open at the end of an
older log is stored `aborted` and is replaced if its END ever arrives. Stored:
raid bosses, arena matches, keyed runs' Σ (their member bosses — any boss at keystone difficulty 8, START seen or not — only under `history_store_trash`) and
plain visits' Σ; Trash only under the switch; noise never. Retention per
(kind, encounter | map, difficulty), oldest first, never the protected set
(pinned, annotated, the fastest kill, the owner's best per_sec per spec for
damage and healing). "Me" is `history_characters`, else the one guid every
stored log's COMBATANT_INFO named. `Status` carries a `HistoryStatus` (v20).

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
`status`, `list_fights`, `fight`, `breakdown`, `compare`, `loadout` are answered
from one lazily connected `ClientKind::Mcp` daemon session — the daemon is
spawned only when a tool first needs it, failures are tool-level errors, and
each fight tool call is answered from the first snapshot matching the cursor it
declares (a client like every frontend; `loadout` rides `GetLoadout` (v19) and
reports the player's COMBATANT_INFO talents + gear, naming the talents through
the dataset via the same picks→encode→decode path as the GUI's logged view, raw
picks when the dataset is absent, and `logged: false` — not an error — when the
log holds no build at or before that segment; the player resolves against the
damage rows, then the healing rows, and every follow-up request is pinned to
the id the first snapshot resolved `Live` to); talent tools
`talent_tree`, `decode_talents`, `encode_talents` answer from the per-machine
talent dataset (R14), never the daemon.

## Dependencies

model: zero-dep. core, proto, daemon: stdlib only. mcp: stdlib only (JSON is
hand-rolled like the wire codec — parse never panics; the value type and the
talent codec live in proto — `proto::json` / `proto::talents` — so the gui's
talent viewer reads the same code, and mcp re-exports them; `proto::history` is
the history store's record codec, one JSON document per file, shared by the
daemon that writes and every reader that parses). tui: ratatui + crossterm.
gui: iced + iced_layershell + serde/toml. history: model + proto + duckdb
(SYSTEM-linked to nixpkgs' libduckdb, the crate version pinned to the
library's; never `bundled` — signed off 2026-09-02 for roadmap item 1, the one
analytical engine in the tree, and it lives in the `wowdps-history` binary
only, never in the daemon). Everything else stdlib unless justified and signed
off. No chrono (hand-parse the timestamp), no tokio (threads +
channels), no serde outside the gui.

Dev-dependencies (tests only, never linked into a binary): the gui may use
iced's own test harness and software renderer (`iced_test`, `iced_tiny_skia` —
headless rendering of every screen and canvas) plus the in-repo `wowdps-daemon`
(its `mock` over the fixture) and `wowdps-core`; mcp likewise uses the daemon
and core. The gui binary still links model + proto only and cannot parse a log.

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
