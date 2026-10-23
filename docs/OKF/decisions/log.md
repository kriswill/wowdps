# Log

## 2026-10-23

- **Creation** — [DuckDB Is Bundled, Not System-Linked](duckdb-is-bundled-not-system-linked.md):
  `wowdps-history` compiles DuckDB from the crate's own source on every
  platform (JSON and Parquet, no ICU), dropping the pin to nixpkgs'
  libduckdb that held the crate a release behind and kept three zip
  advisories open; no DuckDB debug info in debug builds, dependabot held
  to DuckDB 1.x.

## 2026-10-19

- **Creation** — [An Import Carries Its Meter](an-import-carries-its-meter.md):
  v48 — an imported fight's players carry the totals the importer reckoned
  under the rulings and their 1 s damage and healing curves; the store
  derives the rates over the card's combat clock and keeps the curves as
  the details tier, still no rows or series.

- **Update** — [Imported Fights Live In The History Store](imported-fights-in-the-store.md):
  its "replay tier alone" amended by v48's meter and details tier.

- **Update** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  an import is a card, a replay and (v48) its details.

## 2026-10-17

- **Update** — [Keep A Keystone Run's Affixes](keep-keystone-affixes.md):
  after review — a key joined mid-run reads NULL in the lake too, and every
  older replay is recut, keyless ones included, rather than re-stamped.

- **Creation** — [Keep A Keystone Run's Affixes](keep-keystone-affixes.md):
  v47 and replay format 6 — the affix ids a CHALLENGE_MODE_START lists are
  parsed onto the keyed visit and kept on the card, the replay head, the
  mcp card and the lake, landed early so a regrade and the recut queue
  backfill every stored key while its log survives.

## 2026-10-12

- **Update** — [Give The Map A Vocabulary Per Level And Per Area](map-vocabulary-per-level-and-area.md):
  after review — altitudes filled from the view before they are checked
  (`Area::altitudes`), an ordered crop, fade, court and spell checks, an
  arena's own `travel`, a levels fixture, Echo of Doragosa's court posted
  on 2097 though its draft says 2099; and the base rule narrowed to what
  holds: a set may amend an arena's map and view, never a level's.

- **Creation** — [Give The Map A Vocabulary Per Level And Per Area](map-vocabulary-per-level-and-area.md):
  a dungeon level is an instance file's `[level.<ui_map>]`, the same `Map`
  an arena's is; any map names its areas (with altitudes), paths (with a
  mode) and a transition, and the render's crop, include, fade, ground and
  exposure; all base, every resolved map held to origin/main's.

## 2026-10-11

- **Update** — [Imported Fights Live In The History Store, Apart From Ours](imported-fights-in-the-store.md):
  the replay goes through `write_replay`, the size cap trimming only once
  the card lands.

- **Update** — [Imported Fights Live In The History Store, Apart From Ours](imported-fights-in-the-store.md):
  after review — an import must be this build's replay format (a format
  names the cut's rules, and an import is never recut); a failed write
  changes nothing (a re-import's old replay put back); a busy refusal is
  no dropped write; the TUI claim and the commit hashes (now PR #108)
  corrected.

- **Creation** — [Imported Fights Live In The History Store, Apart From Ours](imported-fights-in-the-store.md):
  v46 — a fight converted from outside this machine's logs is stored by
  the daemon as a card naming its origin and its replay tier alone,
  protected like a pin, listed only on request (`OriginFilter`) and never
  read by the store's judgments of this machine's history.
- **Update** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  links the import decision: the tier also holds imported fights.

- **Update** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  format 5 — a pool reads as the game's bar: the parser takes a cast's cost
  off (R28's series moves too) and the cut advances a pool by the energizes
  its target's block never shows; measured on a +15 night's warlock and
  paladin. An energize starts only a counted pool (max below 100: shards,
  holy power, combo points …), never a continuous one no block named (a
  Mana Spring on an elemental shaman), the coordinator's review call.

## 2026-10-10

- **Update** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  format 4 — an aura up before the pull counts R18's way, opened at t 0 (the
  user's call); bounded pools (power type below 64, eight kinds a post);
  `encode` writes tidied pools; the format number moves so the queue recuts
  format 3 too. A pet's debuffs on NPCs are a known gap.
- **Update** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  format 3 adds each post's shields and every pool kind, the buffs that
  matter to the fight (R18's curated kinds or an NPC's, never one up before
  the pull) and every debuff a player puts on an NPC — the rows a followed
  unit's HUD reads; two real +15 runs grew 11.7 and 12.1 %, and the rewrite
  queue recuts formats 1 and 2.
- **Update** — [Creature Classifications Come From The Addon, Kept As One TSV](creature-classes-come-from-the-addon.md):
  the amendment adds that a file that exists but cannot be read is refused,
  not rewritten.
- **Update** — [Creature Classifications Come From The Addon, Kept As One TSV](creature-classes-come-from-the-addon.md):
  amended for outside writers — a seed tool, a hand edit or another
  machine's sync may write `creatures.tsv`, and the daemon re-reads it and
  folds it in by the newer-wins rule before each merge, so their rows
  survive.
- **Creation** — [Creature Classifications Come From The Addon, Kept As One TSV](creature-classes-come-from-the-addon.md):
  the addon records `UnitClassification` and `UnitIsLieutenant` by creature
  id from nameplates (the log and `Creature.db2` lack them); the daemon
  merges them into `creatures.tsv` at the store's root, a contracted file
  the replay reads; it lags a logout.
- **Update** — [Guilds Come From An Addon, Joined At Read](guilds-come-from-an-addon.md):
  links the creatures section the same addon took on.

## 2026-10-09

- **Update** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  format 2 keeps every floor's posts and the boss rows, so a keystone run
  replays through its whole dungeon (three real runs lost 36 to 59 % of
  their posts to the single floor); the rewrite queue recuts format 1.
- **Update** — [devenv Is The Reference Shell](devenv-is-the-reference-shell.md):
  the packages' fileset leaves out the Markdown under `crates/` and
  `tools/extract/`, which no build or test reads, so an `AGENTS.md` edit
  moves no package hash.
- **Update** — [devenv Is The Reference Shell](devenv-is-the-reference-shell.md):
  the rest of the environment — the nightly toolchain, the GUI's need for a
  shell, the `nix/dev/` parcels and the wrappers, the flake's packages and
  modules — moved here from the agent instructions.
- **Creation** — [The Replay Tier Lives In The History Store, Cut By The Parser](replay-tier-in-the-store.md):
  R29 cuts every boss pull and keystone run from its parsed lines into a
  binary replay tier (`replay/<id>.bin`), kept on the details caps and all
  season for the protected set; the replay spike's text cutter is rewritten
  onto the parser; PROTO_VERSION 45 (`GetReplay`).
- **Creation** — [A Progression Wipe Is Kept Whole Until The First Kill](progression-kept-whole.md):
  a wipe on a boss no stored card killed at that difficulty is kept whole as
  a kill is, until the first kill there; `history_keep_progression_whole`.
- **Creation** — [Resolve An Encounter Through An Ordered Layer Stack](encounter-rubric-layers.md):
  the encounter rubric, brought to main as `wowdps-encounter-rubric` with
  the base alone, resolves through base, curated and user tiers: the base's
  four sources, each curated set in the order laid (embedded, then
  bundles), the user's files; difficulty overrides last; a provenance per
  key path.
- **Creation** — [A Pool, Second by Second](a-pool-second-by-second.md): new
  ruling R28 keeps each player's power as a 1 s series per type, drawn under a
  healer's graph; R27's energize reaches the live drill and R26 counts
  empowered stages; PROTO_VERSION 44 for the three and the heal-absorb split.
- **Creation** — [A Heal-Absorb's Part Is Healing, Shown Apart](a-heal-absorb-is-healing.md):
  R2 counts what a heal-absorb ate as healing done and surfaces it as
  `heal_absorbed`, capped at what the line healed. Follow-up, not taken:
  `SPELL_HEAL_ABSORBED` (the absorb's own line) still parses `Other`.

## 2026-10-08

- **Creation** — [Armor Is Mitigation](armor-is-mitigation.md): R17's record
  keeps what armor and damage reduction took off (`reduced`, from the damage
  suffix's `unmitigated`), inside `mitigated_pct`; PROTO_VERSION 43, one bump
  for the 2026-10-08 parser series.

## 2026-10-04

- **Creation** — [A Σ Knows Only What Its Members Knew](a-sigma-knows-its-members.md):
  `Meter::overall` builds from `Segment::empty`, and identity statements land
  only in an open segment, so a guardian owned after a key's END no longer
  folds into a full replay's Σ alone (65,856 damage on a real +13).
- **Creation** — [An Open-World Door Is Zoned Out](an-open-world-door-is-zoned-out.md):
  a ZONE_CHANGE onto a map Map.db2 calls open world reads as difficulty 0, so
  a hearth logged as `"Silvermoon City",15` no longer opens a city visit and
  splits the raid's; the generated `open_world_maps.rs` names the maps.
- **Creation** — [A Pull Zones In The Door That Logged Zero](a-pull-zones-in.md):
  an ENCOUNTER_START while zoned out opens or resumes its visit (a raid
  re-entered after a Heroic→Mythic switch logged its doors at 0, and the
  Mythic pulls fell outside any visit while the Heroic one stayed live);
  such a START is a seed, replayed through the new `Meter::seed`.
- **Creation** — [devenv Is The Reference Shell, And The Flake Shell Mirrors It](devenv-is-the-reference-shell.md):
  devenv for local builds and CI; the flake shell mirrors devenv's clang
  and linker to the same store path, PKG_CONFIG_PATH pinned in both,
  sccache wraps rustc, the contract checks the mirror — cargo had rebuilt all
  567 crates on every switch while the two drifted. CI keeps both cargo
  caches in main's scope (cache-warm.yml) and PRs only read them.
- **Creation** — [A Fight Replay Captured Once And Interpreted Per Tier](fight-replay-captured-once.md):
  roadmap item 2a's research plan — a replay mode of the pull view built on
  the positions every advanced line carries, captured once in the background
  and kept for good, mechanics, phases and verdicts re-interpreted per tier,
  local only, a sticky off switch.
- **Creation** — [Scope The Replay's Game Data To The Season The Client Names](replay-data-from-the-season.md):
  draft — the current season resolved from the keystone pool and the
  journal's "Current Season" tier, checked against the user's logs; encounter
  data keyed by difficulty through the client's fallback chain, rooms never;
  per-machine caches; the hotfix cache an open gap.
- **Creation** — [Draw The Replay's Room From The Game's Own Floors](replay-floors-from-game-files.md):
  draft — the dungeon map per floor for v1, its transform verified on a real
  kill; the minimap render where floors don't stack; a render of the floor's
  WMO groups as the route to sharp rooms; prior art compared, nothing borrowed.
- **Creation** — [The Replay's Room Turns To The User, Without Edges](replay-room-turns-without-edges.md):
  plan decision 8 — a view rotation with a default from the room's walls and
  the raid-to-boss bearing, the user's angle kept per encounter; the floor
  resampled on the CPU because GPUI draws images axis-aligned.
- **Update** — [No Forked Or Patched GPUI](no-gpui-forks.md): the replay's
  rotated floor as another design-around.

## 2026-10-03

- **Creation** — [Boss Kills And Timed Keys Are Kept Whole, And A Stored Pull Answers As A Live One](stored-pulls-kept-whole.md):
  v42 — every boss kill and timed key protected for the season, the caps
  counting the unprotected alone; series format 2 keeps each ability's
  targets, its whole-fight tallies and the 1 s damage taken; a stored drill
  stacks, opens an ability and compares byte for byte as live through
  `GetFight.spell` / `pair`; stale kept fights rewritten from their logs at
  start.
- **Update** — [Stored Pulls Keep Their Seconds In A Binary Series Tier](a-series-tier-for-stored-windows.md):
  points at v42, which keeps the per-ability targets it measured and left out.
- **Creation** — [A Death Ends Where They Are First Seen Alive](a-death-ends-at-first-sight.md):
  v41 — R23 ends a death at the rez or the first advanced-block health report
  above zero, never at damage dealt in their name (a real +15's imps
  imploded 65 ms after their warlock died); an Overall carries a wipe's open
  deaths into the next member; `Mark::open` on the wire; the inspector draws a
  death and its return as red and green rules, the hatch gone.
- **Update** — [An Inspector Beside The Meter](inspector-beside-the-meter.md):
  its hatched death replaced by the two rules.
- **Creation** — [Themes Are Data A Config Can Name, And Onyx Is The Second](themes-as-config-data.md):
  a registry of named themes (navy — the old gold —, onyx, frost) chosen in the
  ⚙ card, every token group a config.toml table that overrides a built-in or
  defines a theme of one's own; corners as one scale, effects off in navy; onyx
  the black-dial chronograph with its own faces, glass, brackets and sub-dial.
- **Update** — [Gold Chrome, Tabular Barlow, And A Look Per Surface](gold-chrome-and-a-look-per-surface.md):
  its gold chrome is now navy's own accent, `chrome = "theme"`.

## 2026-10-02

- **Creation** — [Count And Clock The Way The Game's Own Meter Does](match-the-game-meter.md):
  v40 — overkill leaves every amount (R1), friendly fire leaves the Damage row
  (R22), and every rate divides by an engagement-based combat clock (R7/R10),
  after a reconciliation against the game's built-in meter on a real +15
  traced each difference to one cause; the run rate a key keeps beside it.

## 2026-10-01

- **Creation** — [Stored Pulls Keep Their Seconds In A Binary Series Tier](a-series-tier-for-stored-windows.md):
  v39 — a fourth, binary history tier of per-second abilities and targets for
  kills, keys and pinned fights, read one player at a time, windowed through
  the live drill's own function; measured sizes and the retention rule.
- **Update** — [A Graph's Window Scopes The Drill](a-window-scopes-the-drill.md):
  its stored-pull gap closed by the series tier.
- **Creation** — [A Graph's Window Scopes The Drill, And The Inspector Widens](a-window-scopes-the-drill.md):
  v38 — a drag on a Damage or Healing drill's graph re-asks for its abilities
  (and Damage's targets) inside the window from the sparse series, every list
  saying whether it kept the window, the bump with no byte moved; and the
  inspector's corner button (`f`) widening it over the stage.
- **Update** — [An Inspector Beside The Meter](inspector-beside-the-meter.md),
  [Enemy Taken Is A Live View](enemy-taken-is-a-live-view.md): link the v38
  decision that grows them.
- **Update** — [Rebuild The GUI On GPUI Kit, Beside The Iced One](gui-on-gpui.md):
  its outcome — parity and cost measured against iced, what the decision did
  not foresee, and the cutover (`e72791d`, `e6ff075`), merged as #71 on the
  user's sign-off.
- **Update** — [No Forked Or Patched GPUI](no-gpui-forks.md): its crate is
  `wowdps-gui` now, built as gui-new.
- **Update** — [Gold Chrome, Tabular Barlow, And A Look Per Surface](gold-chrome-and-a-look-per-surface.md):
  what the cutover kept (every rule) and retired (`Look::OVERLAY` / `WINDOW`,
  the `_in` twins, `line_icons.rs`, `ellipsis.rs`, the hash guard).

## 2026-09-30

- **Update** — [Rebuild The GUI On GPUI Kit, Beside The Iced One](gui-on-gpui.md):
  the `gui_binary` key and the dev unit's stamp of it landed (plan step 0.3).
- **Creation** — [Rebuild The GUI On GPUI Kit, Beside The Iced One](gui-on-gpui.md):
  gui-new on GPUI Kit's styled layer beside the iced crate until a measured
  cutover, framework-free logic moved into `gui-logic`, one theme definition
  for Kit's `Theme` and the app's `Look`, Kit's click-through harness.
- **Creation** — [No Forked Or Patched GPUI](no-gpui-forks.md): stock
  gpui-pre through gpui-kit only; a gap is designed around or sent upstream.
- **Creation** — [A Hit A Shield Took Whole Is A Hit](whole-absorb-is-a-hit.md):
  R1's absorbed-part convention applied to the whole hit, on both sides of R17's
  identity, `prevented` redefined so `mitigated_pct` holds, segmentation untouched.
- **Creation** — [A Talent Proc Nests Under Its Driver](proc-under-its-driver.md):
  a curated table proven from both sides, the census's two metrics, what was
  left out and why, and the v37 bump.
- **Update** — [The Ability Tree In The Inspector](ability-tree-in-the-inspector.md):
  step 4 links the proc decision.

## 2026-09-29

- **Update** — [The Ability Tree In The Inspector](ability-tree-in-the-inspector.md):
  step 3, Miss % and a DoT's uptime.
- **Update** — [The Ability Tree In The Inspector](ability-tree-in-the-inspector.md):
  step 2, the drill graph stacked by the same entries in six validated hues.
- **Creation** — [The Ability Tree In The Inspector](ability-tree-in-the-inspector.md):
  a player's abilities nest by summon, trinket and (id, periodic) part, the
  daemon deciding (R26, PROTO_VERSION 36) and the window folding them inside
  its five columns; casts in the opened ability's numbers.

## 2026-09-28

- **Update** — [One Fight Header](one-fight-header-over-the-meter.md),
  [An Inspector Beside The Meter](inspector-beside-the-meter.md),
  [One Pull Rail](one-pull-rail.md),
  [The Raid Timeline And Whose Rows On The Wire](raid-timeline-and-mine-on-the-wire.md)
  and [One Table For Every Row List](one-table-for-every-row-list.md) link back
  to the Redesign decision; the Wire record names the one `Window`-kind
  connection that is sent a timeline it never reads (the overlay's Σ split).
- **Creation** — [Redesign The Window From A Prototype, Reviewed In Headless Shots](window-redesign.md):
  the redesign as a whole — gold chrome over class chrome, one pull rail,
  master and detail, the headless design shots and the overlay guard as the
  review loop — linking each step's record, and the Home and palette step,
  which has none of its own.
- **Update** — [Home Derives Client-Side From Fights](home-derives-from-fights.md):
  amended by the redesign's Home step — a week read one page at a time, not
  a list grown by scrolling.
- **Update** — [Gold Chrome, Tabular Barlow, And A Look Per Surface](gold-chrome-and-a-look-per-surface.md):
  `Look::type_scale` and the 800 px meter width left with the Inspector step.
- **Update** — [The Raid Timeline And Whose Rows On The Wire](raid-timeline-and-mine-on-the-wire.md):
  the timeline is built only for the sessions that use it (window, mcp).
- **Creation** — [The Raid Timeline And Whose Rows On The Wire](raid-timeline-and-mine-on-the-wire.md):
  the window redesign's Wire step — PROTO_VERSION 35 with R25's raid
  timeline on every meter snapshot, the recap's time before death and a
  per-row `mine` from the daemon's owner resolution.
- **Creation** — [One Pull Rail For Tonight And Every Stored Night](one-pull-rail.md):
  the window redesign's Rail step — a top bar and one pull rail over the log
  and the store, deduplicated by fight id, and stored pulls drawn by the
  meter's own renderers through a `ClientState` fed from `GetFight`.
- **Update** — [One Table For Every Row List](one-table-for-every-row-list.md):
  its History screen is retired by the Rail step; the table stands.
- **Creation** — [An Inspector Beside The Meter](inspector-beside-the-meter.md):
  the window redesign's Inspector step — master and detail fed by an opt-in
  follow-selection in `ClientState`, lanes coloured by caster under a plot
  that shares their x, a comparison overlaid inside the inspector, and one
  Esc chain ending at Home.
- **Creation** — [One Fight Header Over The Meter](one-fight-header-over-the-meter.md):
  the window redesign's Header step — one title line and one stat line with
  the "you" chip, the filter in the tab row, the view's own fourth column,
  and a total row that follows a short list and pins under a long one.

## 2026-09-27

- **Creation** — [Gold Chrome, Tabular Barlow, And A Look Per Surface](gold-chrome-and-a-look-per-surface.md):
  the window redesign's Look step — gold chrome by default, the tabular-baked
  window fonts, no semantic yellow, and `theme::Look` with `_in` twins so the
  overlay's shared renderers keep their pixels.
- **Update** — [One Chrome Accent, Split By Luminance](one-accent-from-the-class-color.md):
  refined by the gold chrome; the rule now derives only `chrome = "class"`.

## 2026-09-21

- **Creation** — [Retire A Log When The Tailer Leaves It, Never When The Game Exits](retire-a-log-on-tail-switch.md):
  a `Switched` retires the previous log to the history thread, which imports its
  open tail as the start-up sweep would; the game-process signal closes nothing.

- **Creation** — [Cooldown Windows Belong To The View They Move](cooldown-windows-per-view.md):
  v34 adds `HealingCooldown`, the R18 table covers every spec (eight-log census),
  and a GUI graph draws only the marks its view is about.

## 2026-09-14

- **Creation** — [Enemy Taken Is A Live View](enemy-taken-is-a-live-view.md):
  R24's view is wire (PROTO_VERSION 32) but never stored; VIEW_KEYS is a fixed seven.

## 2026-09-13

- **Creation** — [One Table For Every Row List, And History As A Window-Local Browser](one-table-for-every-row-list.md):
  GUI slice 2 — the table primitive, the surface-keyed `?` sheet, the History
  screen, the death navigator, the Taken cards and the R21 matrix.

## 2026-09-09

- **Creation** — [Guilds Come From An Addon, Joined At Read](guilds-come-from-an-addon.md):
  wire v31, the `addon/` Lua, `proto::lua`, and the store's `affiliations/`.

## 2026-09-05

- **Creation** — [Keep the knowledge bundle under docs/OKF](okf-bundle-under-docs.md).

## 2026-09-06

- **Creation** — [Keep Self-Harm Off The Damage Meter](self-harm-off-the-damage-meter.md):
  R22, found by diffing a real +14 against the group's in-game meters.

## 2026-09-07

- **Creation** — [Home Derives Client-Side From Fights](home-derives-from-fights.md):
  the GUI refresh's slice 1, with the daemon's read quota and page cap as its
  prerequisites.
- **Creation** — [One Chrome Accent, Split By Luminance](one-accent-from-the-class-color.md):
  the light/dark ink rule, calibrated against Warrior tan.
