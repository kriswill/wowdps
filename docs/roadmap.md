# Roadmap

Where wowdps goes next, in order. Each item builds on the ones above it;
the order is deliberate. Earlier plans (`plan.md`, `plan-client-server.md`)
remain the record of what already shipped and why.

Ground rules that every item inherits: CONTRACT.md stays binding (a wire-shape
change bumps `PROTO_VERSION`), the dependency policy holds (model zero-dep;
core/proto/daemon stdlib only), and the daemon never becomes an event store —
it stores *summaries* it can derive, not raw events.

## 1. History store + analytics (daemon, then MCP)

**Why first.** The game writes a fresh `WoWCombatLog-*.txt` per session and the
daemon tails only the newest one, so today history ends at the last login. This
is the single biggest blind spot, and everything below (analysis views, coach
grades, progression graphs) needs a place to keep data across sessions.

**Shape.**

- When a segment closes (encounter, keyed visit's Overall, arena match), the
  daemon writes a *fight summary* under `$XDG_DATA_HOME/wowdps/history/`:
  identity (encounter/map id, difficulty, visit ordinal, arena flag, verdict,
  start time, duration, official key time), the rows per `View`, per-player
  breakdowns, the R12 timeline buckets and markers, the R9 death recaps, and
  each player's logged loadout (v19 COMBATANT_INFO talents + gear).
- Serialized with the existing `wire.rs` primitives, one file per fight plus a
  small append-only manifest for listing — no new format, no serde in the daemon.
- A few KB per pull. Trash is not stored by default (config key), noise
  segments never.
- Idempotent on replay: a fight already stored (same log identity + byte range)
  is not written twice, so restarts and rescans are safe.
- Retention is by count per encounter (config key), best kills pinned.

**Analytics the store enables** (computed on read, in the daemon, answered over
the wire and via MCP):

- best kill per boss + difficulty, per spec and overall;
- pulls-to-kill and best-percent progression per boss;
- DPS / HPS trend per character + spec over time;
- keystone time trends per dungeon + level.

**Delivery.** Daemon store first, then MCP tools (`history`, `best`, `trend`,
`progression`) so the coach skill consumes it immediately. GUI views come in
item 2. New `ClientMsg`/`DaemonMsg` variants = `PROTO_VERSION` bump.

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
