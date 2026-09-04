# Roadmap

Where wowdps goes next, in order. Each item builds on the ones above it;
the order is deliberate. Earlier plans (`plan.md`, `plan-client-server.md`)
remain the record of what already shipped and why.

Ground rules that every item inherits: CONTRACT.md stays binding (a wire-shape
change bumps `PROTO_VERSION`), the dependency policy holds (model zero-dep;
core/proto/daemon stdlib only), and the daemon never becomes an event store —
it stores *summaries* it can derive, not raw events.

## 1. History store + analytics (daemon, then MCP)

**Status: shipped 2026-09-02, hardened 2026-09-03** — every step of
`docs/spec-history-store.md` §13, R16 included (`best_pct` on the card, per
night in `progression`), on PR #12; then a code review and fourteen rounds of
retests against a real store by the coaching session, which added paging,
the owner's grade on every card, roles, difficulty names, one fight id
everywhere, `stored_fight` tiers and the keyed-boss drill, a pin-preserving
`regrade` command, local nights, and three refinements to R16. The spec's
status paragraph lists what differs from the sketch below — read it first;
the bullets under "Spec:" are the original outline.

**Why first.** The game writes a fresh `WoWCombatLog-*.txt` per session and the
daemon tails only the newest one, so today history ends at the last login. This
is the single biggest blind spot, and everything below (analysis views, coach
grades, progression graphs) needs a place to keep data across sessions.

**Spec:** `docs/spec-history-store.md`. The short version:

- A local data lake. When a fight closes (encounter, arena match, keyed visit's
  Overall) the daemon writes per-fight JSON documents under
  `$XDG_DATA_HOME/wowdps/history/v1/` with `proto::json`: a ~400 B card, the
  six views' rows plus death recaps, and for kills, bests and pinned fights a
  details file with breakdowns and timelines. Loadouts are content-addressed.
  Trash is off by default, noise never.
- The daemon stays stdlib-only. It answers the fixed questions (fights, best
  kill, progression, trend, key times) from an in-memory index of the cards,
  over three new one-shot wire messages. `PROTO_VERSION` 20.
- Ad hoc analytics live in a new `wowdps-history` binary that embeds DuckDB
  (system-linked from nixpkgs, never bundled) over the same files. The MCP
  server proxies the fixed questions to the daemon and shells out to that
  binary for a `history_sql` tool. This is the one new dependency (signed
  off: nixpkgs DuckDB 1.5.4, system-linked).
- Prerequisites in core: encounter id + difficulty on segments, the game build
  from `COMBAT_LOG_VERSION`, and the timestamp's timezone offset. Best-percent
  progression rides on ruling R16 (boss health), built with the store.
- Fight identity is the log's header line hash plus the segment's start
  millisecond. Idempotent on restart, rescan and replay.
- Retention by count per encounter, with a protected set: pinned, annotated,
  fastest kill, and the owner's best per spec. Annotation records are reserved
  for item 4.

## 1a. Role pivots: healer and tank analytics (follow-on to item 1)

**Status: planned, 2026-09-03.** A separate project after item 1; nothing
here is started. Written down now because the store's analytical model
(`docs/history-store-design.html` §9) serves DPS questions for every fight
and healer or tank questions only partly — and the tank half is a parser
gap, not a storage gap.

**Why.** Healers want effectiveness (overhealing, wasted absorbs, absorbs
given, externals given and received, uptime of the buffs they maintain) and
a ranking among healers, not among DPS. Tanks want damage taken by ability,
mitigation (block / dodge / parry / partial blocks / absorbs consumed),
active-mitigation uptime, self-healing, and who the boss was hitting (the
nearest thing to threat the log can express). Today every ranking and grade
the coach produces is a DPS-role number.

**Where the data is today.**

| Want | Held today | Where | Gap |
| --- | --- | --- | --- |
| Effective healing, overheal | yes, every fight | `rows` Healing view: amount = effective, `extra` = overheal (R2) | none — a SQL ratio |
| Absorbs given | yes, folded | SPELL_ABSORBED credits the absorber as healing (R3); split per shield spell only in `details` (kills) | not separable on the card / rows |
| Wasted absorbs (expired unconsumed) | no | nothing tracks a shield's applied value vs consumed | parser: aura applied/removed + the absorb-amount field on aura lines |
| Healing received per player | dead players only | R9 recap ring (32 events) | no per-player "taken" grain |
| Healer ranking, share, median | no | MCP grades among DPS-role players; role is a reader-side spec lookup | role on the card's player rows |
| Externals given / received | Bloodlust family + Power Infusion, receiver only | `MarkKind::External` on timelines (kills) | a defensive-external table with caster AND target |
| Buff / uptime | no | nothing stores aura spans | aura spans for a curated set |
| Damage taken by ability | dead players only | recap ring | a per-player Taken grain on the rows tier |
| Block / dodge / parry / miss | no | `SWING_MISSED` / `SPELL_MISSED` → `Event::Other`; partial blocks on damage lines dropped | parser + a new View |
| Absorbs consumed on a tank | healer's side only | R3 puts the amount on the absorber | the Taken grain carries `absorbed` |
| Stagger / cheat-death | excluded | `NON_HEALING_ABSORBS` (114556, 31850, 31230, 115069) are dropped from healing | Taken grain keeps them as mitigation |
| Self-healing | kills only | `details` heal_targets where target = source | rows-tier column, or the Taken grain's `healed_self` |
| Active-mitigation uptime | no | nothing | aura spans, same table as externals |
| Threat / boss target | no — not in the log | — | proxy: boss-sourced damage per player from the Taken grain + R16's boss identity |

**How it maps into the analytical model** (§9 of the design document: the
card is the wide fact row, `rows` and `details` hang off its id).

- **Role dimension.** `CardPlayer` gains `role: Option<Role>` (Tank /
  Healer / Dps) from the spec, stamped at write time so SQL and the daemon
  agree; `regrade` back-fills. The MCP `me` block then grades among
  same-role players and adds `rank_hps` / `hps_median` for healers.
- **Taken grain, on the rows tier for every fight.** A seventh view or a
  parallel `taken` list: fight × player × source spell with `amount`,
  `absorbed`, `blocked`, `overkill`, `count`, and miss counts by type
  (dodge / parry / block / miss / immune). Healer effectiveness reads the
  Healing view; tank mitigation reads Taken; soaks and avoidable-damage
  markers (item 2) read Taken too. This is the same "damage-taken breakdown"
  the spec's §14 lists as refinement 6, promoted from `details` to `rows`
  because wipes are where tanks die.
- **Healing split on the card.** Two more measures per player: `absorbed`
  (shield healing) and `overheal`, so effectiveness and absorb share are
  card-only queries like DPS trend is today.
- **Aura spans with caster and target.** A generated per-spec table (like
  `class_spells.rs`) of active mitigation, personal defensives and healer
  externals; the meter records them as timeline marks with `dur_ms` (the
  mark already carries a duration) plus a `src` guid, so "externals given"
  is a group-by over marks by source and "mitigation uptime" is a sum of
  durations over the fight. Rides the cooldown-mark refinement (spec §14
  item 2) — one table, two roles' questions.
- **Wasted absorbs** is the one item needing new parser state: pair
  `SPELL_AURA_APPLIED` (with its trailing absorb amount) to the shield's
  consumed total from SPELL_ABSORBED and its removal. Deferred until the
  Taken grain exists; it is a ruling change (R2/R3 stay, a new R adds
  "shield applied" as a measure) with fixture parity.
- **Rulings.** Miss events and partial blocks are new parser events and a
  new View, so CONTRACT gains a ruling for Taken (what counts, whose row,
  how absorbs and stagger land on it) with fixture expectations, and the
  scanner stays untouched (none of these open a segment). Wire: trailing
  fields only until the new View, which is a `PROTO_VERSION` bump.

**Order.** (1) role on the card + same-role grading (no parser change,
immediate coach value); (2) the Taken grain (parser events + ruling + rows
tier + `history_sql` view); (3) healing split on the card; (4) aura spans
via the generated table; (5) wasted absorbs. Each is shippable alone.

## 2. GUI analysis views

**Why second.** The parser already models interrupts, dispels, auras, casts,
absorbs, overheal and deaths, but the GUI renders six views plus Compare.
Most of what follows is rendering over data the model already carries, so it
does not touch CONTRACT.md rulings.

In order of payoff:

- **Death recap screen.** R9 data exists on the wire and nothing draws it.
  Last N damage events, healing received, defensives used, per death.
- **Damage taken by ability**, with an avoidable-damage marker (a generated
  table, like `item_spells.rs`, is the honest way to flag avoidable spells;
  start hand-curated per tier).
- **Cooldown and buff uptime bars** on the timeline graph, from
  `SPELL_AURA_APPLIED`/`REMOVED` and `SPELL_CAST_SUCCESS`.
- **Boss phase markers** on the compare graph, so the timeline has landmarks.
- **History graphs** over item 1: best-kill table, progression per boss,
  DPS trend per spec, keystone trends.
- **Replay scrubbing.** The R12 timeline already buckets per second; a slider
  shows the meter as it stood at second N.
- **Share/export.** One key renders the current screen to PNG (the headless
  render path the tests use already exists) and exports a fight summary as JSON.

Overlay gets only what fits its surface (death recap, markers); the rest is
window-only, like the talent viewer.

## 3. Settings page + config reload

**Why here.** Cheap, but it needs a daemon-side change: the daemon reads
`~/.config/wowdps/config.toml` once at startup, so editing `logs_dir`,
`game_process`, `auto_overlay` or the history keys from a page would require a
restart. Fold this in when item 1 or 2 first needs a daemon-owned setting.

- Window-local screen (like the talent viewer), never a model `Screen` variant.
- GUI keeps writing the toml; the daemon keeps its section-aware stdlib reader.
- New `ClientMsg::ReloadConfig` (reserved in `plan-client-server.md`); the
  daemon re-reads, re-targets the tailer if `logs_dir` changed, and answers
  with `Status`. `PROTO_VERSION` bump.
- Every key gets a live preview where it can (overlay edge/offset/zoom already
  apply on the fly).
- The LLM/coach keys from item 4 live here.

## 4. Coach pane

**Principle.** The GUI is the coach's *display*, not its brain. The hard part of
coaching — the tool loop, the rubric, same-spec comparisons, trend tracking —
already exists as the `wow-coach` skill over the MCP server, with the whole
tool surface. Rebuilding it inside the GUI would mean an HTTP + TLS stack in a
crate that has neither, plus a second agent loop to maintain.

**Shape.**

- MCP tools that let an agent *write*: `grade_fight` (score, verdict, findings)
  and `note` (free text against a fight or a character), stored in the history
  store from item 1 next to the fight they grade. The store already
  reserves `annotations/<id>.ndjson` and protects annotated fights from
  retention; `docs/spec-history-store.md` §14 lists what the coach's first
  real report needed from the store and ranks the refinements (marks on the
  rows tier, major-cooldown and defensive marks, annotations, item names) —
  read it before shaping these tools.
- A coach pane in the GUI window renders grades and notes for the selected
  fight, plus a trend of grades over time.
- A "grade this pull" action shells out to the Claude Code CLI in print mode
  with the MCP server attached, and the pane refreshes when the grade lands.
- **Later, behind the settings page:** a direct API mode (provider, model,
  key) for users without the CLI. Only once the display and storage exist,
  and only with a minimal client — no agent framework.

## 5. Distribution: Linux first, Windows if demanded

**Linux packaging.** The project's niche is Linux players under Proton, and the
install story is Nix-only. Flatpak, AUR and a plain release tarball reach every
other distro for a fraction of a port's cost. Do this before Windows.

**Windows.** The most expensive item and the weakest fit: the overlay is
wlr-layer-shell, game detection is a `/proc` sweep, the socket is unix-only in
~50 places, and the stdlib-only daemon cannot enumerate processes on Windows
without a crate or a shell-out. Windows players also have in-game Details!,
so the sell there is the MCP coach, not the meter. If demand shows up:

- honest scope is daemon + windowed GUI + MCP, **no overlay**;
- a small `ipc` module (unix socket / named pipe) behind `DaemonClient` and
  `server.rs`; `PROTO_VERSION` is unaffected if the frame layout is;
- `game.rs` grows a Windows backend (`tasklist` shell-out keeps the daemon
  stdlib-only);
- CI gains a Windows build in the canary lane before anything is promised.

## Not planned

- Raw event storage / a WCL-style upload target.
- inotify (polling is fine and rotation-aware).
- Solo Shuffle round splitting (noted in R13; revisit if arena users ask).
