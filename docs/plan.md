# Fast startup: index the log, lazy-load encounters, auto-attach live

## Context

Today `wowdps` replays the entire log through the full parser+meter before it is
useful. The current real log is 329 MB / 1.1M lines / 23 boss pulls (57 segments),
and replay is throttled by design: `main.rs` drains the tail channel ≤25 ms per
frame (`DRAIN_BUDGET`), then blocks up to 200 ms in `event::poll` — ~10% parsing
duty cycle, i.e. minutes of watching bars simulate.

Prior-art research (WarcraftLogs uploader, Details!, ACT, wowparser) converges on
one pattern: **index the file once at I/O speed, show the fight list, and fully
parse only what the user opens**. The structural markers make this cheap — an
`ENCOUNTER_END` line already carries the success flag and fight duration, so the
encounter list needs no damage parsing at all. Measured on the real 329 MB log: a
boundary-only scan completes in well under half a second; a boss pull is a
3–15 MB byte slice that parses in a few hundred ms.

Nobody rotates the file the game has open (retail already starts a fresh
`WoWCombatLog-MMDDYY_HHMMSS.txt` per session), so no rotation/archiving is
needed — with an index, unopened history costs nothing.

## Goals

1. App start → segment list on screen in <1 s for a 300 MB+ log.
2. Selecting a segment parses only its byte range and opens the existing meter UI.
3. If a fight is in progress at startup, skip the list and land on its live meter.
4. Live tailing keeps working exactly as today once attached.

Non-goals (explicitly out of scope, possible later): persistent sidecar index,
WCL-style offline log splitting, Details-style retention caps, inotify.

## Design overview

The reader thread's job changes from "replay everything as lines" to:

```
open file → scan forward building an index (boundary markers + byte offsets)
          → emit Index(Vec<SegmentMeta>)
          → emit Lines(...) from the last segment boundary onward (live meter seed)
          → keep tailing as today
```

The app gains a second screen (segment list) and keeps a small cache of lazily
parsed historical meters. The live `Meter` is fed only from the last boundary
onward, so it is useful within seconds.

`CONTRACT.md` interfaces for `parser.rs`/`meter.rs` are untouched. `Meter`,
`Segment`, `Row`, `parse_line` all stay as-is — they just get fed lazily.

## Changes by file

### New: `src/index.rs` (~the only new module)

```rust
pub struct SegmentMeta {
    pub kind: SegmentKind,          // reuse crate::model::SegmentKind
    pub name: String,               // encounter name or "Trash"
    pub start_ms: i64,
    pub end_ms: Option<i64>,        // None only for the trailing open segment
    pub success: Option<bool>,      // from ENCOUNTER_END's success field
    pub duration_ms: i64,           // R7 semantics (see parity note below)
    pub byte_range: (u64, u64),     // [start, end) offsets into the file
}

pub struct Index {
    pub segments: Vec<SegmentMeta>,
    pub live_offset: u64,           // where the reader should start emitting Lines
    pub scanned: u64,               // bytes consumed by the scan
}

/// Single forward scan. Reads in large chunks, splits lines, and for each line
/// looks at only: the timestamp prefix and the event token after the "  "/tab
/// separator. Full CSV parsing happens for no line.
pub fn scan(reader: impl Read) -> Index;

/// Read one segment's raw lines for lazy parsing. Plain seek + bounded read.
pub fn load_range(path: &Path, range: (u64, u64)) -> io::Result<Vec<String>>;
```

Scanner rules — deliberately a mirror of `Meter::feed`'s segmentation so that
index-then-lazy-parse and full-replay produce identical segments:

- `ENCOUNTER_START` closes any open segment, opens an Encounter one.
- `ENCOUNTER_END` closes it; success + duration come straight off the line
  (duration = ENCOUNTER_START..END per R7; the line's own fightTimeMs field is a
  cross-check, not the source of truth).
- `COMBAT_LOG_VERSION` mid-file = hard boundary (R6).
- Combat events outside an encounter accrue to a Trash segment; a >60 s gap in
  combat events starts a new one (reuse `TRASH_GAP_MS` from `meter.rs` — export
  it). Trash duration = first..last combat event (R7).
- "Combat event" must match what actually calls `Meter::record`: the
  damage/heal/absorb/interrupt/dispel/death event-name sets already in
  `parser.rs` (reuse/export the `is_damage_event`-style matchers), plus
  `SPELL_AURA_APPLIED` **only when** the aura is a DEBUFF and the spell id is in
  `CC_SPELLS` (export from `meter.rs`; extract the spell-id field with a cheap
  comma scan, no allocation). This exactness is gated by the parity test below.
- Timestamps: reuse `parser::parse_timestamp` on the prefix before the
  separator (make it `pub(crate)`), same "  "/tab split as `parse_line`.
- `live_offset` = byte offset of the last segment boundary (start of the
  trailing open/most-recent segment). Everything from there is emitted as
  normal `Lines` so the live meter contains the in-progress segment from its
  first event, including its `COMBATANT_INFO` lines when it's an encounter.

### `src/tail.rs`

- Add `TailEvent::Index(index::Index)` (or a struct payload with the segments +
  the path). Emitted once per file, after `Switched`, before any `Lines`.
- `retarget()` currently opens at offset 0 and the reader replays everything.
  Change: after opening, run `index::scan` on the file up to EOF-at-that-moment,
  emit `Index`, seek to `live_offset`, then continue the existing chunked
  `Lines` behavior from there. Truncation/inode-change handling (`read_open`'s
  metadata check) is unchanged — a retarget just rescans (cheap).
- Directory rotation to a newer file: unchanged; the new file gets its own
  `Switched` + `Index`.
- Scan runs on the reader thread, so the UI can render a "scanning…" frame
  meanwhile (the existing `Waiting`/`Switched` events already drive the header).

### `src/app.rs`

- New screen state:
  ```rust
  pub enum Screen { List, Meter }
  ```
  `App` gains: `screen`, `index: Vec<SegmentMeta>`, `list_sel: usize`,
  a bounded cache `loaded: Vec<(usize, Meter)>` (LRU, cap ~8 — each parsed
  slice holds per-actor hashmaps; 57 of them is needless memory), and the
  existing `meter` becomes the **live** meter fed by the tail.
- Selection model on the Meter screen: a selected segment is either
  `Loaded(cache idx)` or `Live`. `App::segment()` resolves through that instead
  of always `self.meter.segments()[seg_sel]`.
- `on_tail` handles `TailEvent::Index`: store it, then decide the start screen:
  **live detection** = the index's trailing segment is an open encounter, OR its
  last combat event is within `TRASH_GAP_MS` of the scan end *and* the file
  mtime is recent (a few seconds). If live → `Screen::Meter` pinned to the live
  segment (exactly today's behavior); else → `Screen::List`.
- Keys: on `Screen::List`, `j/k`/arrows move, `Enter` opens the selected
  segment, `q` quits. On `Screen::Meter`, everything as today, plus `Esc` with
  no drilldown open returns to the list; `[`/`]` walk the index and lazily load
  neighbors (same load path as `Enter`).
- Loading stays out of `app.rs` (it is pure/no-I/O by design): `app` exposes
  `wants_load: Option<usize>` (set by `Enter`/`[`/`]`), and **`main.rs`**
  performs `index::load_range` + feeds a fresh `Meter`, then calls
  `app.install_loaded(idx, meter)`. Synchronous is fine: a 3–15 MB slice is a
  few hundred ms in release; draw one "loading…" frame first (reuse the
  `status` footer for it).
- As the live meter closes segments (encounter end / trash gap / R6), append
  matching entries to the index list so the list stays complete without rescan.

### `src/ui.rs`

- `draw()` branches on `app.screen`: new `draw_list` renders one row per
  `SegmentMeta`, newest last, selection marker like `draw_rows`:
  `  12  Midnight Falls        Kill   1:03   21:00` (rank, name, Kill/Wipe/—,
  duration via existing `duration()`, start time-of-day from `start_ms`;
  Trash rows render dimmed, `Color::DarkGray`, Details-style "trash is
  disposable" emphasis). A `LIVE` row at the bottom when a segment is open.
- Header on the list screen: file name + "N segments"; footer gets list hints
  (`j/k move | enter open | q quit`).
- Meter screen header/footer unchanged except `esc back` hint also applies
  outside the drilldown.

### `src/main.rs`

- The drain loop is unchanged (the budget now only ever throttles the live
  tail, which it was designed for).
- After `app.apply(action)`, service `app.wants_load` as described above.

### `src/model.rs` / `CONTRACT.md`

- Re-export `index` types through `model.rs` like the rest.
- `CONTRACT.md` needs a coordinator-signed addition: `src/index.rs` (owner:
  core) with the `SegmentMeta`/`scan`/`load_range` signatures, the
  `TailEvent::Index` variant, and the two-screen keybind additions
  (list screen; `Esc` = back-to-list). Meter/parser sections unchanged.

## Semantics parity and accepted tradeoffs

- **Parity is a tested invariant**: for `fixtures/sample.txt` (and the larger
  fixture logs), scanning + lazily parsing every slice must yield segments with
  identical (kind, name, start/end, success, duration) to a full-replay
  `Meter`, and identical `rows()` for every view. This gates the scanner's
  combat-event classification.
- **Pet ownership across slice boundaries**: `Segment::new` seeds owner maps
  from earlier segments during full replay; a lazily parsed slice loses
  pre-slice `SPELL_SUMMON`s. Advanced logging (which this log has) carries
  `ownerGUID` on the damage lines themselves (`owner_hint` in the parser), so
  attribution still resolves. Accepted, same tradeoff WCL's splitter makes;
  worst case a pre-summoned pet without advanced fields shows as its own
  breakdown row in a *historical* slice. The live meter is unaffected.
- **Class colors in historical slices**: encounters include their own
  `COMBATANT_INFO`; trash slices may lack one, so bars can be colorless there —
  same as today before the first `COMBATANT_INFO` arrives.

## Implementation order

1. `index.rs`: `scan` + `load_range` + `SegmentMeta`; export `TRASH_GAP_MS`,
   `CC_SPELLS`, event-name matchers, `parse_timestamp` as `pub(crate)`.
   Unit tests + the fixture parity test.
2. `tail.rs`: scan-on-retarget, `TailEvent::Index`, tail-from-`live_offset`.
   Extend the existing temp-dir tests (index emitted once, lines start at the
   boundary, rotation rescans).
3. `app.rs`: `Screen`, list state, load cache, live detection, key handling,
   `install_loaded`. Tests via `testkit` fixtures (list nav, enter-opens,
   esc-returns, live-jump, `[`/`]` lazy walk).
4. `ui.rs`: list screen rendering + TestBackend tests (rows, ordering, dim
   trash, LIVE row, hints).
5. `main.rs`: `wants_load` servicing; `CONTRACT.md` + `plan.md` housekeeping.

## Status

Implemented 2026-07-28. Measured on the real log (355 MB, 57 segments, 23
pulls): scan 345 ms; biggest pull (23 MB) lazy-loaded and parsed in 171 ms.
One improvement over the plan: instead of accepting the cross-slice pet/class
tradeoff, the scanner records byte ranges of the rare state-carrying lines
(`SPELL_SUMMON`, `COMBATANT_INFO`, `COMBAT_LOG_VERSION`) as `SegmentMeta::seeds`
and `load_segment` prepends them — lazy parity is exact, classes included.
Run the perf gate anytime with:
`WOWDPS_REAL_LOG=<log> cargo test --release -- --ignored real_log --nocapture`

## Verification

- `cargo test` — including the new parity test (index+lazy == full replay on
  fixtures) and all existing suites untouched.
- Real-log smoke test: `cargo run --release -- --file <329 MB log>` → list
  visible in <1 s; select a boss pull → meter in <1 s; totals for one pull
  spot-checked against the current full-replay build on the same file.
- Live test: `cargo run --release` (default logs dir) while appending to a copy
  of the log with a script (or during actual play): verify live detection lands
  on the meter, and an `ENCOUNTER_END` in the tail appends a list entry.
