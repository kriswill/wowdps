# Role pivots, step 4a — implementation plan (R18 aura spans: the role-spell table, then spans with caster and uptime)

Scope after the devil's-advocate review (log at the end). Step 4 of
`docs/spec-role-pivots.md` §11 lands in three PRs: **4a-i** the curated,
generator-validated role-spell table (a reviewable diff of ids); **4a-ii**
ruling R18 in the engine (spans with caster and target, capped spans plus
an uncapped uptime rollup, the taken series R17 deferred), the fixture,
and the wire bump the new mark kinds and the caster force (`PROTO_VERSION`
24); **4b** the rows-tier `uptime[]` / `coarse[]`, the card measures and
the store / MCP / DuckDB side — a store change only, no bump. Branch
`role-pivots-6` off `role-pivots-5` (PR #21).

## Facts (explorers + real logs, 2026-09-04)

- R12: `note_mark(player = dst, spell, ts, cast)` with veto order ①
  `EXTERNAL_BUFFS` → `External`, ② `class_spells` veto, ③ `item_spells`;
  dedupe (own cast within 2 s, same label within 500 ms, re-apply while
  open); `close_mark` closes the newest open mark; open marks read 0;
  marks keyed by the target, the caster discarded at the call site.
  **`MARK_CAP` drops the newest, not the oldest** (`note_mark` and
  `absorb`). The three mark call sites reach `segments.last_mut()` with
  **no `end_ms` guard** — a buff after `ENCOUNTER_END` lands in the closed
  segment past its end (invisible today as a mark; a span would read
  negative). `SPELL_AURA_REFRESH` is unparsed; it shares `APPLIED`'s
  13-field shape (FORMAT-NOTES has no aura row).
- Real logs: on every boss pull the tanks' first Shield Block / SotR is an
  `APPLIED` 2–6 s after the start — the "buff predated the segment" case
  never fires on encounters and is really the pre-pull-cast-before-trash
  case. Cap pressure: the busiest player has 22 curated auras per 5-min
  pull (594–687 auras of all kinds), so the curated set is what keeps 256
  sane; a 50-min key is ~330 Shield Blocks alone, so the list wraps on
  keys and the rollup must be the gated measure. Bone Shield: 95 REFRESH
  to 9 APPLIED (refresh-while-open absorbs them).
- **Cast ids are not aura ids.** Metamorphosis' buff is 162264 (the cast
  191427 never appears as an aura), Arcane Surge's is 365362, Bladestorm's
  446035, Voidform both 194249 and 228260, Mystic Touch 113746; Summon
  Infernal has no aura at all. A name check passes every one of them.
- `SupportBuff` debuffs (Chaos Brand, Hunter's Mark, Mystic Touch) land on
  Creatures, never on a player: no path to a span under a
  target-is-a-player rule. Ebon Might / Prescience / Shifting Sands are
  player-targeted (152 / 64 / 79 in the Augmentation session).
- Extractor: generators are `TABLES` of hardcoded FileDataIDs and a
  `generate()` emitting a sorted static table with a sortedness test and a
  3-line header; `SpellEffect` (with `Effect` and `EffectAura`) is already
  a known table; `SpellCategories` / `SpellCooldowns` are not in the tree.
  Nothing gates class/spec in the meter for marks, and nothing would.
- Wire: `Mark` bytes `at_ms, kind, label, spell_id, dur_ms`; a `src`
  string adds ~23 B per mark (256 marks: 16 KB vs 10 KB per timeline) —
  acceptable, consumers are the MCP `caster` key, the compare hover, and
  the coach's "given to whom". The compare legend is a fixed four-key row.

## 0. Decisions

- **Table (4a-i)**: membership is a curated list in `rolegen.rs` of
  `(id, expected name, kind)`; the generator (a) fails unless the id has a
  `SpellEffect` row with `Effect == 6` (APPLY_AURA) — an id that never
  produces an aura line is a build failure, which a name check cannot give
  — and unless its `SpellName` matches; (b) emits `(id, kind)` only — **no
  class/spec gate** (no consumer); (c) emits `role_spells.expected.md`
  with an `observed` column from a committed census of the two real logs
  (raid + Augmentation session) so a guess is visible in review. Only
  census-exercised ids ship; the coach requests the rest. `Cooldown` stays
  (its live consumer is the compare graph's burst bar, the twin of the
  External bar) with its exercised ids. Cut: Chaos Brand, Hunter's Mark,
  Mystic Touch (debuffs on enemies), Blistering Scales, Breath of Eons,
  Serenity (removed from the game), Summon Infernal (no aura), and every
  unobserved id.
- **R18 (4a-ii)**: a Buff `AuraApplied` / `AuraRefresh` on a player whose
  spell is in the table opens a span keyed by the target with the caster
  as `src`, consulted only for `!cast`, **before** the class-spells veto
  (the table replaces `EXTERNAL_BUFFS`' slot), **bypassing the item dedupe
  rules** (own-cast-within-2 s and same-label-within-500 ms are trinket
  semantics); re-apply while open is a refresh. `AuraRemoved` closes the
  newest open span. **A refresh or removal with no open span opens one at
  the segment's start** (the buff predated the segment: the only way a
  refresh can precede an apply inside it). **All three mark call sites go
  through `open_segment_for_passive`** (the 2b predicate), so a buff after
  a segment's end lands nowhere and lazy = full. **The close at segment end
  is computed at read time** in `marks_for`, kind-branched: a role span
  still open reads `min(end, now) − at`, an item mark still reads 0 (a
  proc that never dropped is not a span; no R12 golden moves); a Trash
  segment has no close event in its byte range, so a mutate-on-close could
  never be parity-safe. Spans live in their own list under `SPAN_CAP =
  256` (a tank's spans cannot evict a trinket proc), inheriting R12's
  newest-dropped rule, stated in CONTRACT. **`uptime`: an uncapped rollup
  per target per `(spell, src)` of `{count, total_ms}`** is the
  fixture-gated measure. Derived on the segment: `am_uptime_pct(guid)` =
  the per-millisecond union of `ActiveMitigation` spans on the player over
  the segment's `duration_ms` (the same duration the card writes — on a key
  Overall that is the timer, not Σ members); `externals_given(guid)` =
  count and total ms of `External` spans with `src` = the player,
  `externals_received` by target; `support_uptime(guid)` = the rollup rows
  of `SupportBuff` spans given by the player, per target. R8 untouched;
  nothing opens or extends a segment; the scanner is untouched.
- **Taken series (4a-ii)**: the damage branch buckets `amount + absorbed` on
  the destination like `series` does on the source; `Segment::taken_timeline
  (guid)` returns it with the player's spans; `Timeline::coarsen(factor)` is
  a pure model helper for 4b.
- **Wire v24 (4a-ii)**: `Mark + src: String` trailing (empty for item
  marks); `MarkKind` codes 4–7 (`ActiveMitigation`, `Defensive`,
  `SupportBuff`, `Cooldown`); a Taken drill's `Breakdown.timeline` is the
  taken timeline. GUI `compare.rs`: legend keys drawn only for kinds
  present in the shown marks; hues — `ActiveMitigation` and `Defensive`
  share one ("mitigation"), `SupportBuff` its own, `Cooldown` in
  `External`'s family; `mark_color` / `mark_name` stay exhaustive; the
  hover reads the caster ("PI from Gennar"). MCP `mark_json` gains the
  names and `caster`.
- **Fixture `spans.txt` (4a-ii)**, ~100 lines, one kill + a trash tail with a
  pre-pull cast: a Protection Warrior (Shield Block spans incl. one
  refreshed inside the pull with no apply — the segment-start rule — and
  one open at the kill; Shield Wall overlapping a Shield Block and a Pain
  Suppression — the union case; a trinket proc, R12 untouched), a Holy
  Priest giving Pain Suppression and Guardian Spirit to the Warrior and a
  Power Infusion to a Mage, an Augmentation Evoker with Ebon Might on both
  and Prescience on the Mage, a Fire Mage with Combustion (cooldown) and
  Alter Time (defensive, observed; Ice Block is not) and a Time Warp
  (external on self and others), a Shield Block cast after `ENCOUNTER_END`
  (lands nowhere), enough hits for a taken series with known buckets.
  Goldens under R18 with `check.awk` computing the union the dumb way (a
  per-second bitmap), `externals_given` count and ms, `support_uptime_ms`,
  taken spot buckets; every expected TSV regenerates, pre-existing metrics
  byte-identical. The committed table is what the goldens assume.
- **Real-log gate (4a-ii)**: balanced spans per (target, spell) (at most one
  open at end), tank AM uptime within 0–100 % with the union never over
  100 %, healers' externals given > 0, no negative durations, and the
  census of table ids seen.

## 1. Order and agents

1. **4a-i (delegated)**: `model::RoleSpellKind`; `tools/extract/src/rolegen.rs`
   (+ CLI, `pub mod`, `tools/gen-role-spells.sh`), unit tests on synthetic
   CSVs incl. the fail-loud cases (no APPLY_AURA effect; renamed), the log
   census script/committed counts, run against the install (network for
   `.dbd`s and keys), commit `role_spells.rs` + `role_spells.expected.md`
   + FORMAT-NOTES' aura rows. PR stacked on #21.
2. **4a-ii, parallel after 4a-i**: (A) parser `AuraRefresh` + meter R18 +
   taken series + `Timeline::coarsen` + core tests + the real-log gate;
   (B) fixture + goldens + awk; (C) model `Mark.src` / kinds + wire v24 +
   proto goldens + GUI/MCP arms + CONTRACT R18 row and v24 row.
3. Adversarial diff review per PR, fixes, PRs stacked.

Estimate: 4a-i ~2.5 k (2 k of table), 4a-ii ~3.5 k.

## Review log (devil's advocate, 2026-09-04)

Verdict *rethink the validation and the segment-edge rules, then ship*.
Blocking: a name check passes cast ids that never produce an aura line
(Metamorphosis, Arcane Surge, Bladestorm, Summon Infernal…) — the
generator now requires an APPLY_AURA effect and the expected file carries
a real-log census (B1); three `SupportBuff` entries were debuffs on
enemies with no path to a span — cut (B2); the mark call sites have no
`end_ms` guard and a mutate-on-close rule would go negative and break
trash parity — passive gate plus read-time close (B3). Should-fix: the
cap drops the newest, not the oldest — stated, and spans get their own
cap (S1); the segment-start rule never fires on encounters, only before
trash — kept, gated (S2); role kinds bypass the trinket dedupe (S3); the
class gate had no consumer — cut (S4); the legend decided (S5); split
4a-i / 4a-ii (S6). Nits: the union is the headline and the per-spell
rollup the drill; externals as count and ms; `src` as a string; the
Overall's duration; the awk union as a bitmap.

## Second review log (adversarial diff review of 4a-ii, 2026-09-04)

Verdict *open after fixes*, all applied. Blocking: the mitigation union
was an incremental busy counter that re-counted closed groups when a
second mitigation spell was first seen by refresh (a Blood DK's Bone
Shield after a Blood Shield proc) — replaced by uncapped per-target
intervals swept at read time, each clamped at the segment's close, with
the reproducers (B1); open spans keyed by (target, spell) let two casters
of one spell fabricate a span from the segment start — keyed by (target,
spell, caster) with the segment-start rule firing once per key (B2).
Should-fix: the trash-tail exception for externals and support totals is
now stated in CONTRACT and the AM union never exceeds a segment on any
kind (S1); deleting `EXTERNAL_BUFFS` had silently dropped the three
hunter lusts the census never saw — they ride the table census-exempt
with the reason emitted into the expected file (S2, 64 entries); the
coach reference names the new kinds and `caster` (S3); the legend keys a
span drawn in the window, not only one starting in it (S4); the real-log
gate's tautological bound is replaced by list ⊆ rollup and the AM bound
on every segment (S5). Nits: the awk's pet comment, `close_span` credits
the opening caster, span dedupe in the Overall merge, `coarsen`'s partial
tail.
