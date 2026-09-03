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
  store from item 1 next to the fight they grade.
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
