# crates/core (wowdps-core)

The engine: parser, meter, index, tail, the replay cut and the generated
tables. Only the daemon runs it. Binding text: `CONTRACT.md` (signatures,
rulings, §Fixtures). Read the ruling before changing what it governs.

## Rules

- **The contract moves with the code.** A behavior change updates the
  ruling in CONTRACT.md, the goldens (`fixtures/*.expected.md` / `.tsv`),
  `fixtures/check.awk` and the code in one change.
- **The parser never fails a line.** An unknown event is `Event::Other`;
  decode never panics (`tests/parser_fuzz.rs`).
- **Scanner and meter in lockstep.** `index.rs`'s scan mirrors
  `Meter::feed`'s segmentation. A line that opens or extends a segment in
  one must do so in the other. A line kept off the Damage row (R22's
  self-harm and friendly fire) still runs `ensure_combat`: the scanner
  counts damage lines without reading amounts.
- **Lazy equals full.** A segment opened through `load_segment_text` +
  `SegmentText::meter` equals a full replay. State carried across segments
  arrives as seed lines through `Meter::seed`, never `feed` (a visit seed can
  be an `ENCOUNTER_START`); keep `seeds()` and `slice()` apart. New
  cross-segment state needs a seed and a lazy = full test.
- **The passive gate.** Auras, casts, energize, resurrects, power reports,
  support and every timeline mark go through it and never open or extend a
  segment. Neither does R17's Taken ledger.
- **Raw keys, folded at read.** Per-actor ledgers key on the raw guid and
  fold onto owners when read. An Overall merges its members' ledgers.
- **Self-harm folds by summon only** (`Meter::summon_fold`), never through
  the ownership map: a charmed mob would swallow a player's damage (R22).
- **Game-meter parity.** An amount is `amount + absorbed − overkill`; every
  rate divides by `Segment::combat_ms` (R1, R7). A key's `duration_ms` stays
  the key timer.
- **R8 inference** is segment-local, overwritten by COMBATANT_INFO, and
  never opens a segment.
- **R30 role inference** (`roles.rs`) is segment-local too: its ledger is
  written only for an edition with a role-signal table (Forever), from the
  fight's own lines, never seeded, never carried, never opening a segment.
  Retail's role is its spec's: no Retail segment keeps one. The weights are
  constants in `roles.rs`; `check.awk`'s R30 block holds the same ones.
- **R31's logger** (`Segment::logger`) is segment-local too: each unit
  block flagging the logging character (`Unit::is_logger`: Mine,
  player-controlled, a `Player-` player, no pet or guardian type) counts
  for the segment open once its line is read, through the passive gate;
  never seeded, never carried, never opening a segment. `check.awk`'s
  `logger` metric mirrors it.
- **The replay cut is passive** (`replay.rs`, R29): it moves no meter
  number and shares the meter's parse (`cut_and_meter`). Core cannot name
  proto's `placed` table; the daemon implements `PlacedTable` over it.
- **Generated tables are never hand-edited.** `class_spells.rs`,
  `item_spells.rs`, `keystone_timers.rs`, `open_world_maps.rs`, `instance_maps.rs`,
  `role_spells.rs`, `absorb_spells.rs`, `proc_spells.rs` and Forever's
  `role_signals.rs` come from `tools/gen-*.sh` (read `tools/AGENTS.md`). `class_spells` wins over
  `item_spells`' generous trigger chase.
- **Each table is one edition's.** Retail's sit in `src/`, Forever's in
  `src/forever/` (class, item, keystone, open-world, party-and-raid-map, absorb, role-span, and
  the role signals Retail has none of). Code outside tests
  reads every table through `tables.rs` with the fight's edition
  (`Segment::project_id`): an edition with no table of a kind gets nothing
  of it, never another edition's entry, except open-world maps, which an
  edition with none of its own reads from Retail's for now. A new
  edition's table is one entry there (R8). An Overall reads its first
  member's edition, never the meter's latest version line.
- **A wire or store shape change** goes through `crates/proto/` and its
  rules; a meter fix that moves no shape keeps `PROTO_VERSION`.

## Fixtures

- Each `fixtures/*.txt` is a synthetic log with hand-computed goldens, and
  `check.awk` recomputes them from the log alone, with no parser. Never
  derive a golden from the meter's output.
- A new modeled metric gets a `check.awk` metric and its rows in every
  gated fixture's TSV.
- A new fixture brings its `.expected.md` and `.tsv`, a `check.awk` block,
  a test under `tests/` and a CONTRACT.md §Fixtures entry.
- Invent every name: no real player, guild or realm.
- `fixtures/FORMAT-NOTES.md` documents the log format.

```sh
crates/core/fixtures/verify.sh                 # every gated fixture vs its TSV
crates/core/fixtures/verify.sh crates/core/fixtures/corrupt.txt   # must FAIL
cargo test -p wowdps-core
WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt WOWDPS_REAL_LOG_FOREVER=/path/to/_forever_/Logs/WoWCombatLog-*.txt \
  cargo test --release -p wowdps-core -- --ignored real --nocapture    # every gate, both editions
WOWDPS_REAL_LOG_FOREVER=/path/to/_forever_/Logs/WoWCombatLog-*.txt cargo test --release -p wowdps-core -- --ignored forever --nocapture
```

The real-log gates need a log with at least one closed segment
(`docs/tracing.md`, "Real-log gates"). libtest filters on a test's own
name, never its file's: every gate's name holds `real`, and every Forever
variant's `forever`; `real_log` selects four. Each gate whose rule
Forever has (the whole-file parse, deaths, taken, stacks, power, shields,
spans, player labels, the logger, the boss names, and roles,
`real_log_segments`' cut and totals and `real_log_posts`' replay positions,
which are Forever's alone) has a
Forever variant over `WOWDPS_REAL_LOG_FOREVER` (`tests/real_log/mod.rs`);
a variant prints and asserts on no name and no guid, a failure included,
and a Retail gate passes a Forever log over. A frozen live meter is usually the
game's multi-minute flush: check the log's mtime before debugging
`tail.rs`.
