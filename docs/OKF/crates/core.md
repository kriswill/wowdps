---
type: Crate
title: wowdps-core
description: 'WoW combat-log engine: parser, meter, structural index, file tailer.'
resource: crates/core
tags: [crate]
status: stable
generated: { by: okflight/0.4.0, at: 2026-09-06T18:29:58-07:00 }
---

wowdps-core: the engine. Parsing (`parser`), aggregation (`meter`), the startup index (`index`) and log following (`tail`) — everything between bytes on disk and domain rows. Only the daemon runs this; frontends are pure clients binding to `wowdps-model` types over `wowdps-proto`.

## Modules

[`CONTRACT.md`](../../../CONTRACT.md) is the binding text for everything
here: the public signatures of `parser`, `meter` and `index`, every ruling
and the wire. This is a map of where each lives, not a restatement.

- `parser.rs` — one combat-log line → `LogLine` / `Event`; an unknown event
  is `Event::Other`, never an error. Its `HpHint` reads the advanced block:
  health, the unit's absorb (field 9), its primary power (fields 10–12) and
  its position (`HpHint.pos`, fields 14–17). FORMAT-NOTES once named field 7
  absorb; it is versatility, and 8 avoidance.
- `meter.rs` — `Meter::feed` aggregates lines into `Segment`s (Encounter or
  Trash) and answers `Row`s per `View` plus per-player breakdowns;
  `Meter::overall(ordinal)` merges an instance visit's members into a
  synthetic `SegmentKind::Overall` (duration = Σ members'). Its rulings,
  each with a doc: game-meter parity on amounts and the combat clock
  ([R1](../rulings/r1.md), [R7](../rulings/r7.md),
  [Match The Game Meter](../decisions/match-the-game-meter.md)), heal-absorb
  ([R2](../rulings/r2.md)), visits ([R10](../rulings/r10.md)), the
  destination ledger and mitigation ([R17](../rulings/r17.md)), role spans
  ([R18](../rulings/r18.md)), support ([R19](../rulings/r19.md)), shields
  ([R20](../rulings/r20.md)), stacking debuffs ([R21](../rulings/r21.md)),
  self-harm and friendly fire off the Damage row ([R22](../rulings/r22.md)),
  death spans ([R23](../rulings/r23.md)), the enemies' view
  ([R24](../rulings/r24.md)), the raid timeline ([R25](../rulings/r25.md)),
  the ability tree ([R26](../rulings/r26.md)), energize
  ([R27](../rulings/r27.md)) and the power series ([R28](../rulings/r28.md)).
  Passive events (auras, casts, energize, resurrects, power reports) go
  through the passive gate and never open or extend a segment.
- `index.rs` — the structural scan: segment boundaries and byte ranges with
  no per-event parsing, so a 300 MB+ log lists its segments in under a
  second. A segment is parsed only when opened (`load_segment_text` +
  `SegmentText::meter`, a fresh `Meter`), seeded with the earlier
  `SPELL_SUMMON`, `COMBATANT_INFO`, `COMBAT_LOG_VERSION`, R10 visit and
  `WORLD_MARKER_*` lines through `Meter::seed`, never `feed` (a visit seed
  can be an `ENCOUNTER_START`, so `SegmentText` hands out `seeds()` and
  `slice()` apart, never one list) — so a lazy parse equals a full replay.
  The scanner mirrors `Meter::feed`'s segmentation. `Index::checkpoint` /
  `scan_from` make scans resumable, and the daemon's index cache persists
  the checkpoints so a restart rescans only the tail.
- `tail.rs` — `Tailer` follows a file or the newest log in a directory
  (polling ~200 ms, rotation-aware). On open: `Switched` → `Index` (one
  scan, injectable through `with_scan` for the cache) → `Seeds` (through
  `Meter::seed`) → `Lines` from `live_offset`; `CaughtUp` separates the
  backlog from fresh combat.
- `replay.rs` — the replay cut (below).
- `cli.rs` — `default_logs_dir`: config `logs_dir`, else
  `$WOWDPS_WOW_DIR`, else a Steam compatdata scan picking the newest
  `.build.info`; an error only when nothing is found.
- **Generated tables**, each regenerated once per game patch by its
  `tools/gen-*.sh` through [wowdps-extract](extract.md):
  `class_spells.rs` (R8: out of instances COMBATANT_INFO never fires, so a
  class and spec are inferred from casts — segment-local, overwritten by
  COMBATANT_INFO, never opening a segment), `item_spells.rs` (R12's marks:
  trinket uses, procs through a two-level `SpellEffect.EffectTriggerSpell`
  chase, consumables; the chase is generous, so `class_spells` is asked
  first and wins; R26's `trinket_of` names a trinket spell's one owning
  item), `keystone_timers.rs` and `open_world_maps.rs` (R10),
  `role_spells.rs` (R18, curated and census-proven), `absorb_spells.rs`
  (R20) and `proc_spells.rs` (R26, curated and census-proven).

## Gates

- **Fixtures** under `crates/core/fixtures/`, a doc each under
  [fixtures](../fixtures/index.md) and listed in CONTRACT.md §Fixtures.
  `check.awk` recomputes every golden from the log alone, with no parser
  (`verify.sh`; `corrupt.txt` is the negative control and must fail).
  `FORMAT-NOTES.md` documents the log format itself.
- `tests/fixture_totals.rs` holds the meter to the goldens; each ruling's
  own test (`taken.rs`, `spans.rs`, `shields.rs`, `stacks.rs`,
  `support.rs`, `tree.rs`, `raid.rs`, `replay.rs`, `instance.rs`,
  `arena.rs`) holds its structure, lazy = full among it; `real_log_*.rs`
  are the ignored gates over `WOWDPS_REAL_LOG`.

## The replay cut

`replay.rs` (v45, [R29](../rulings/r29.md)) cuts a segment's parsed lines
into a `model::replay::Cut` — one reading of the log for the meter and the
replay alike: the parser's `HpHint.pos` and the passive replay events feed
it, the index seeds every WORLD_MARKER line, and the module doc lists each
deliberate difference from the extractor's old text cutter
([why](../decisions/replay-tier-in-the-store.md)). Every post keeps its
floor (a keystone run walks a dungeon's levels; the cut's `floor` is the
players' most posted) and the slice's ENCOUNTER lines are boss rows; gated
by [replay.txt](../fixtures/replay.md) (its keystone run too) and every
fixture's R29 rows.

## Source

- Manifest: [`crates/core/Cargo.toml`](../../../crates/core/Cargo.toml)
- Root: [`crates/core/src/lib.rs`](../../../crates/core/src/lib.rs)

## Contract

Public signatures and dependency policy: [`CONTRACT.md`](../../../CONTRACT.md).
