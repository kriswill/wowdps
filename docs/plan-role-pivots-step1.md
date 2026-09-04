# Role pivots, step 1 — implementation plan

Scope: delivery step 1 of `docs/spec-role-pivots.md` §11, cut down after a
devil's-advocate review (log at the end). What ships: the role written into
every card's player rows, a role-relative `me` / `peer` grade in the MCP
(a healer ranked among healers by HPS), a `role` column on the SQL
`players` view that works on un-regraded lakes, a `role_ranks` view that
applies the daemon's exclusion floors, and the lake parity gate widened to
compare the two graders. **No parser change, no wire-shape change, no new
card fields beyond `role`, no retention change.** `wire::Reader::opt` reads
a presence byte and `finish()` rejects trailing bytes, so any new field on
`HistoryQuery` is the `PROTO_VERSION` 21 bump — that is step 2, and
`Fights { role }`, `Trend { measure }`, `RoleNight`, the `roles` /
`has_support` card fields, `Spec::support` and the `tank_pair` block all
move there or later (see the spec's §11).

Branch: `role-pivots-1` off `main`. One PR, merge commit.

## 0. What already exists (don't rebuild)

- `model::Role` + `Spec::role()`; every MCP roster row already emits `role`
  from the spec at read time (`mcp/src/tools.rs:1247`, `1847`, `2014`).
- `graded_row` (`mcp/src/tools.rs:1125`) ranks a DPS-role owner's `dps`
  among DPS-role players with two floors (`DPS_FLOOR` against the median of
  the *others*, `DPS_TOP_FLOOR` against the top). The zero-output rule is
  those floors, nothing else.
- `Trend { view: Damage | Healing }` already carries HPS: a healer trend is
  a `view` argument. `wowdps-history trend --healing` matches. Nothing to do.
- `Store::protected()` (`daemon/src/history.rs:1493`) keeps the owner's best
  `dps` *and* `hps` per (group, spec). A healer's best HPS is already
  protected. **Left exactly as is** (review S4).
- The `players` SQL view is a recursive unnest of `players[]`
  (`history/src/lib.rs:197`); a new card field becomes a column with no
  view change (`union_by_name`).
- The card stores `"spec"` as the numeric id and `"spec_name"` as a string
  (`proto/src/history.rs:415`); `from_json` reads the id only.
- The fixture's roster is Arms (71), **Discipline (256)**, Marksmanship
  (254): one healer, no tank. `instance.txt` has no COMBATANT_INFO;
  `arena.txt` carries no spec. So the healer path is fixture-testable
  (a healer set of one), the tank path is hand-built cards only.
- `crates/mcp` is a lib crate (`pub mod tools`); `crates/history` already
  dev-depends on `wowdps-daemon` for the parity gate.
- The store's index is a flat `Vec` scan (`history.rs:1618`) — no
  `by_encounter` / `by_guid` maps despite spec §8. Tests must not assume
  them.

## 1. Model

- Promote the test-private `ALL_SPECS` (`model/src/lib.rs:785`) to
  `pub const Spec::ALL: [Spec; 40]`; the existing test keeps using it.
  Needed by the SQL CASE generator (§5) and a role-total test.
- Test: every entry of `Spec::ALL` has a `role()`; the tank and healer
  sets are asserted by listing (6 tanks, 7 healers) so a patch that adds a
  spec shows in review.
- Nothing else. `Spec::support` waits for R19 (step 3).

## 2. Card (proto `history.rs`)

No new struct fields. Two derived accessors:

```
impl CardPlayer { pub fn role(&self) -> Option<Role> { self.spec.map(Spec::role) } }
impl FightCard  { pub fn roles(&self) -> RoleCount /* tanks, healers, dps over !enemy */ }
```

`roles()` has no production consumer in step 1 (tests only); it is step
2's `roles` card field arriving as an accessor, kept because it is four
lines and step 2 fills the JSON from it.

- `to_json` emits `"role": "tank" | "healer" | "dps"` after `spec_name`,
  `null` when the spec is unknown (a stable column shape for the lake).
  `from_json` ignores the field: the spec is the truth, `role` is a written-down projection for readers that
  cannot call `Spec::role` (DuckDB, anything else on the files). This
  satisfies spec §2.3's "stamped at write time" without a second copy in
  memory (review S1) — spec §2.3's wording is amended to say the file
  carries it.
- Golden JSON: the existing card golden gains the field; a decode of the
  pre-step-1 golden asserts `role()` still answers from `spec`; fuzz
  unchanged.
- `regrade` needs no code: re-extraction rewrites the bytes and the field
  appears. Test (§3) proves it.

## 3. Daemon store

- `extract()` (`history.rs:2003`): no change — `role` is derived from the
  `spec` it already sets.
- **Regrade round-trip test** (`daemon/tests/history.rs`): write the
  fixture through the engine into a `MemBackend` (the existing
  `closed_fights()` + `LogFacts::read()` helpers), then strip `"role"` from
  one stored card's bytes (simulating a PR #12 card), reopen, regrade that
  fight (`Store::regrade` takes a `ClosedFight` + `LogFacts`, the same path
  the existing downgrade-regrade test uses; no daemon, no socket), and assert the bytes now carry `role` for every player with a
  spec and `pinned` survives. Review S8: this is a real test, not "nothing
  new".
- **Owner for the grade tests** (review B3): the mcp server tests already
  spawn a real daemon with `HistoryOptions::characters`, so the new grade
  tests set the owner there; a `Mock::with_owner` was built first and then
  deleted as dead code (second review, S2). The default mock stays
  owner-less and its `me == null` assertion stands.
- `protected()`: unchanged.

## 4. MCP

- Split `graded_row` into a pure, `pub` grading core in `wowdps_mcp::tools`
  (or a new `wowdps_mcp::grade` module re-exported by the lib):

  ```
  pub struct Grade { role: Option<Role>, measure: Option<Measure /* Dps | Hps */>,
                     rank: Option<usize>, count: usize, median: Option<f64>,
                     excluded: usize, share: Option<f64> }
  pub fn grade(card: &FightCard, guid: &str) -> Option<Grade>
  ```

  Policy (the floors) stays in the mcp crate — proto is codec and client,
  and the GUI must not inherit coach rubric constants (review S5). The
  parity test reaches it through a dev-dependency on `wowdps-mcp`.
- `grade` is role-relative: **Dps** ranks `dps` among friendly DPS-role
  players exactly as today; **Healer** ranks `hps` among friendly healers
  with the same two floors; **Tank** returns `measure: None, rank: None,
  count` = number of tanks (no tank measure until R17). `share` is the
  measure's share of the friendly total for that measure (DPS share of all
  damage, as today; HPS share of all healing).
- `graded_row` JSON keeps every existing key with unchanged values for a
  DPS owner (`rank_dps`, `dps_count`, `dps_median`, `dps_excluded`,
  `dps_share`) and adds the generic block on every row: `rank`,
  `rank_measure` (`"dps"` | `"hps"` | null), `rank_count`, `rank_median`,
  `rank_excluded`, `rank_share`. For a Healer owner the `dps_*` keys keep
  today's actual values (`rank_dps` null, but `dps_count` / `dps_median` /
  `dps_share` were always computed over the DPS pool regardless of the
  subject — preserved so old and new answers never flip meaning), and the
  generic block carries the HPS rank. An enemy (arena) subject is never
  ranked and has no share. Tool description explains
  the block and says tanks are unranked until damage taken lands.
- No `tank_pair`, no `healers` array, no `history { role }` (review S3, S7):
  the coach can filter `players: all` rows by their existing `role` key
  without a lying cursor. Descriptions say so.
- Tests over a real daemon with `HistoryOptions::characters` (the mcp
  server-test pattern): owner = the
  Discipline priest → `rank_measure: "hps"`, `rank: 1`, `rank_count: 1`,
  `rank_median` = own HPS (the trivial case asserted explicitly, review
  nit); owner = Arms → generic block equals the `dps_*` block key for key;
  a hand-built card with two tanks → `rank_measure: null`, `rank_count:
  2`; the floors excluding a zero-HPS healer on a hand-built card.

## 5. `crates/history` (SQL)

- `players` view gains `role` **derived by id, unconditionally** — the
  stored value is never read (second review B1: a lake whose stored `role`
  is null in every sniffed row is typed JSON by DuckDB and `coalesce` with
  a VARCHAR CASE then fails; SQL follows the codec and treats `spec` as the
  truth). The probe for a stored `role` column is kept only to `EXCLUDE`
  it from the unnest and to count `cards_without_role`:

  ```sql
  CASE spec WHEN 71 THEN 'dps' WHEN 256 THEN 'healer' … END AS role
  ```

  The CASE is generated at view definition from `Spec::ALL` (id →
  `role().name()`), 40 arms (review B1: `spec` is the id, not the name).
- New view `role_ranks`, with the daemon's floors so it is the daemon's
  grader in SQL (review S6), over friendly players whose role has a
  measure:

  ```sql
  CREATE VIEW role_ranks AS
  WITH m AS (
    SELECT fight_id, guid, name, role, spec,
           CASE role WHEN 'healer' THEN hps ELSE dps END AS measure
    FROM players WHERE NOT enemy AND role IN ('dps', 'healer')
  ), f AS (
    SELECT *,
           max(measure) OVER w                                  AS top,
           (SELECT median(b.measure) FROM m b
             WHERE b.fight_id = a.fight_id AND b.role = a.role
               AND b.guid <> a.guid)                            AS others_median
    FROM m a WINDOW w AS (PARTITION BY fight_id, role)
  )
  SELECT fight_id, guid, name, role, spec, measure,
         CASE role WHEN 'healer' THEN 'hps' ELSE 'dps' END AS rank_measure,
         rank()   OVER w AS rank,
         count(*) OVER w AS count,
         median(measure) OVER w AS median
  FROM f
  WHERE (others_median IS NULL OR measure >= others_median * <DPS_FLOOR>)
    AND measure >= top * <DPS_TOP_FLOOR>
  WINDOW w AS (PARTITION BY fight_id, role ORDER BY measure DESC
               RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING);
  ```

  The two constants are formatted in from the mcp crate's `pub const`s so
  they cannot drift. The daemon includes zero-output players in the
  median-of-others and excludes them by the floors; the view does the
  same (no `measure > 0` pre-filter). `excluded` per fight = `count(*)`
  in `m` minus `count` here; exposed as a column.
- `stats` adds `cards_without_role`: cards whose `players[]` has a
  non-null `spec` and a null `role`, so an un-regraded store is visible
  and `regrade --kind all` has a before/after number.
- Parity test (`history/tests/parity.rs`): for every friendly player on
  every fixture fight, `wowdps_mcp::grade(card, guid)` (dev-dep) equals the
  `role_ranks` row for that guid on `rank`, `count`, `median`,
  `rank_measure`; `players.role` equals `card.role()` for every player;
  and a card with `role` stripped from its JSON still yields the same
  `role_ranks` row (the CASE fallback). The fixture has no player the
  floors exclude, so a hand-written second lake (two cards from a
  `MemBackend` dump, one zero-HPS healer) exercises the floor branch in
  SQL against `grade` too.

## 6. Docs

- CONTRACT.md: no ruling changes. The history record section (if any) gets
  one line for the card's `role`.
- `docs/spec-role-pivots.md`: §2.3 reworded ("the file carries it"); §11
  step 1 narrowed and step 2 widened to match this plan (done with the
  plan, see below).
- `docs/roadmap.md` §1a status line; CLAUDE.md's mcp bullet mentions the
  role-relative `me` block.

## 7. Order of work

1. model: `Spec::ALL` public + role-set test.
2. proto: `CardPlayer::role`, `FightCard::roles`, `to_json` field, goldens.
3. daemon: `Mock::with_owner`; regrade round-trip test.
4. mcp: `grade` core `pub`; role-relative JSON block; tests.
5. history: `players.role` CASE, `role_ranks`, `stats`, parity gate.
6. docs; `cargo clippy && cargo fmt`; full `cargo test` inside
   `nix develop` (DuckDB).

Estimate: ~700 lines including tests (review S9: the earlier 600 covered
twice the scope; the cuts bring it back).

## Review log (devil's advocate, 2026-09-03)

Blocking findings, all confirmed against the tree and fixed above: the SQL
CASE was keyed on spec *names* but the card stores ids (B1); `Spec::ALL`
did not exist (B2); no test pinned the `me` grade and the mock has no owner
(B3). Accepted cuts: derived `role()` instead of struct fields (S1); no
`support` / `has_support` / `roles` card fields until they have a consumer
(S2); no client-side `history { role }` with an unfiltered cursor (S3); no
`protected()` narrowing — it would demote a healer's best-DPS details for
no gain (S4); expose the grader from the mcp lib rather than move policy
into proto (S5); floors in `role_ranks` or the parity gate is a documented
exception (S6); `tank_pair` is a `role = tank` filter over rows the coach
already has (S7). Nothing in the review was rejected.

## Second review log (adversarial diff review, 2026-09-03)

Blocking: the `players` view read the stored `role` through `coalesce`,
which fails on a lake where every stored role is null (an arena-first or
spec-less lake) because DuckDB types the column JSON — fixed by deriving
from `spec` unconditionally, with parity cases for the null-only, mixed and
stripped lakes (B1). Should-fix, all applied: an enemy subject could be
ranked and given a share (S1; `grade()` now refuses, the parity loop asserts
it); `Mock::with_owner` was dead (S2; deleted); `RoleCount` / `roles()` had
no production consumer (S3; kept and justified above as step 2's field);
four doc claims were stale — the spec's "nothing built", "absent" vs the
`null` the codec writes, the plan's "omitted", CLAUDE.md's view list (S4);
the duplicated floor constants stay, pinned equal by a test (S5). Nits
taken: the root re-export of the `grade` fn dropped (module of the same
name), the roster row uses `CardPlayer::role()`.
