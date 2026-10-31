# Log

## 2026-10-30

- **Update** — [wowdps-extract](extract.md): the crate at the replay
  branch's library half (Step 0 of the replay's move to its own
  repository): the game-data library the replay's extractor builds on
  (`command`'s one command line, the pictures, the atlases, the
  per-machine files, the store reader `pulls`, the journal's reading and
  `spelltip`'s text readers), and the generators of files wowdps's crates
  read: proto's `placed` table (`placegen`), the spell tips
  (`spelltipgen`) and the creatures seed (`wowhead`), with `db2sql`
  (`wowdps-db2`). It depends on core and proto, with no features and no
  build script; the replay's half (`wowdps-replay-extract`, with the
  rubric's drafter) lives in wowdps-replay
  ([decision](../decisions/replay-extract-splits-from-extract.md)).

- **Removal** — wowdps-encounter-rubric: the crate left wowdps with the
  replay for the private wowdps-replay repository, its base season files,
  curated set and gen-rubric with it, and on 2026-10-31 the repository's
  history was rewritten without the crate, its earlier `rubric/` tree and
  its record (the format and its curated layer are the paid product's);
  wowdps keeps its `placed` table in [wowdps-proto](proto.md). The
  entries below that name the crate keep their text, their links gone
  with the record.

- **Update** — [wowdps-proto](proto.md) holds the generated `placed`
  table (`placed`, `placed/placed_spells.rs`), moved from the
  encounter rubric; [wowdps-daemon](daemon.md)
  reads it there and no longer links the rubric.
- **Update** — [wowdps-proto](proto.md): `replay::csv::dps_tsv`, the one
  writer of a replay's `dps.tsv` (each player's damage curve on the
  meter's grid).
- **Update** — [wowdps-proto](proto.md), [wowdps-history](history.md):
  `replay::csv::PullFacts` gains `local_ms` and `best_pct`, and
  `pull.txt` says `local` and `best` where they are given;
  `replay-export` gives the card's.
- **Update** — [wowdps-gui-logic](gui-logic.md): the theme's
  `data.tint_white` and `data.tint_ice`, the encounter rubric's white and
  ice on a replay's map, on every built-in theme (`[themes.<name>.data]`
  sets them); no other token moves.
- **Update** — [wowdps-gui-logic](gui-logic.md): `config::write_atomic`
  is public; a link whose target does not exist yet stays a link, the
  first write creating the file it names.
- **Update** — [wowdps-gui-logic](gui-logic.md): `icons::halvings`, a
  picture of any size and its alpha-weighted halvings down to the floor
  `icons::level` stops at.
- **Update** — [wowdps-gui-logic](gui-logic.md): `icons` serves a class
  crest or spec icon at the halving that covers its drawn size
  (`icons::level`, `ClassIcons::sizes`; `class` and `spec` take the size
  in device pixels) and reads tiles lazily; [wowdps-gui](gui.md) asks at
  the zoomed size times the display's scale factor (`W::device`,
  `Ov::device`, the talent viewer's `Paint::scale`).
- **Update** — [wowdps-gui-logic](gui-logic.md): `spell_tips`, the reader
  of the per-machine `spell-tips.bin` (format 2: a spell's tooltip words,
  what a replay's cast lanes ask of it, and the specs whose logs omit the
  press it is the effect of), landed from the replay branch ahead of the
  replay's move to its own repository.

## 2026-10-23

- **Update** — macOS support (PR #111): [wowdps-core](core.md) discovers
  installs under `/Applications` and `~/Applications`;
  [wowdps-daemon](daemon.md) finds the game with `pgrep -fil` there
  (default `game_process` `World of Warcraft.app`), and every other
  system keeps the /proc sweep; [wowdps-gui](gui.md) draws the overlay
  as a floating non-activating panel exactly the content's size,
  re-anchored on every size change, its display chosen by index; the
  [wowdps-proto](proto.md) client runs without `/proc`. DuckDB needs no
  macOS case: it is bundled everywhere (entry below).
- **Update** — [wowdps-history](history.md): DuckDB is bundled from the
  crate's own source (`=1.10506.1`, JSON and Parquet features, no ICU)
  instead of system-linked to nixpkgs' libduckdb; `tests/duckdb_version.rs`,
  which held the pin to the library, is removed, and `network_access_is_off`
  now holds the autoinstall / autoload `SET`s the bundled build's
  compiled-in defaults leave as the only guard
  ([decision](../decisions/duckdb-is-bundled-not-system-linked.md)).
- **Update** — [wowdps-mcp](mcp.md): `history_sql`'s description says there
  are no time zone functions (no ICU) and how to read local time instead.

## 2026-10-19

- **Update** — [wowdps-proto](proto.md), [wowdps-daemon](daemon.md),
  [wowdps-mcp](mcp.md), [wowdps-history](history.md): v48, an import's
  meter — `ImportedPlayer`'s totals and curves, the card's derived rates,
  `ImportedCard::details` written as the details tier by
  `Store::import_replay`, `stored_fight` answering an import's card, the
  lake's `players` rows for an import filled.

## 2026-10-17

- **Update** — [wowdps-daemon](daemon.md): `Supervisor`'s `Drop` runs
  `shutdown`, so a panic unwinding out of `hub::run`, which skips the
  explicit call, still terminates the overlay the daemon spawned. A second
  shutdown is a no-op.

- **Update** — [wowdps-core](core.md), [wowdps-model](model.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md),
  [wowdps-mcp](mcp.md), [wowdps-history](history.md): v47 and replay
  format 6 — a key's affixes from its CHALLENGE_MODE_START on the visit,
  the card's `key`, the replay head and `pull.txt`, the mcp card's
  `instance` and the lake's `fights.affixes`.

## 2026-10-16

- **Update** — [wowdps-daemon](daemon.md): every way out of `hub::run`
  terminates the overlay the daemon spawned (`Supervisor::shutdown`). A
  `wowdps stop` used to drop the supervisor and orphan its child, whose
  reconnect respawned a daemon that spawned a second overlay.

- **Update** — [wowdps-gui-logic](gui-logic.md): `single::claim_overlay`
  exits an evicted overlay itself, and its notice cannot panic. An
  `eprintln!` into a spawned overlay's stderr, a pipe whose reader (the
  daemon) had exited, panicked the acceptor thread before its exit and
  left evicted overlays stacked on the game.

## 2026-10-12

- **Update** — wowdps-encounter-rubric: the render
  keys read — a sidecar's `params` and `made` (`Params`, `RenderOptions`),
  `Map::params_hash`, `Map::level_params`, `Encounter::floor_params`,
  `Floors::stale`; a dungeon level's sidecars (`Floors::levels`,
  `level_renders`); `Map::stencil_keep` and `STENCIL_FEATHER`; every
  season dungeon's terrain level says `ground = "walked"`; `write::ORDER`
  is public.

- **Update** — wowdps-encounter-rubric: after review
  — `Area::altitudes`, `travel` in `INSTANCE_TABLES`,
  `fixtures/resolved-levels.txt`; a set may amend an arena's map, never a
  level's.

- **Update** — wowdps-encounter-rubric: a dungeon's
  levels (`[level.<ui_map>]`, `Level`, `Rubric::level`, `levels`,
  `check_level`) and the map's areas, paths, transition and render tweaks;
  `gate_map`; `fixtures/resolved-maps.txt`.

## 2026-10-11

- **Update** — [wowdps-daemon](daemon.md): an import trims the replay cap
  only once its card is in (`write_replay`), refuses a re-import over a
  replay it cannot read, and keeps a moved import whose annotations it
  cannot carry.

- **Update** — [wowdps-daemon](daemon.md), [wowdps-history](history.md):
  the review's fixes — `restore_replay`, `retire_import`, an ownerless
  import never stamped "me", a busy `ImportReplay` refusal counted as a
  refused read, this build's replay format only; the lake's `role_ranks`
  leaves imports out.

- **Update** — [wowdps-history](history.md), [wowdps-mcp](mcp.md): v46
  imports — the lake's `fights.imported` / `players.imported`, the fixed
  questions on `NOT imported`, `stats.kept.imports`; the mcp's card
  `origin` and an import's word in `stored_fight`.

- **Update** — [wowdps-proto](proto.md), [wowdps-daemon](daemon.md): v46
  imported fights — `history::Origin`, `ImportedCard`, `imported_id`;
  `ImportReplay` answered `ImportedReplay` / `Refused`; `OriginFilter` on
  `Fights`; `kept_imports`; `Store::import_replay` and the judgments that
  skip imports; `Status` now carries the v45 replay and kept counts.

- **Update** — [wowdps-core](core.md), [wowdps-proto](proto.md),
  [wowdps-daemon](daemon.md): replay format 5 — the parser's `Power::cost`
  (block field 13) taken off a cast's current, which moves R28's series too;
  the cut's energize readings (`display_mod`; only a counted pool,
  `COUNTED_POOL_MAX`, started by one); the rewrite queue recuts
  formats 1 to 4.

## 2026-10-10

- **Update** — [wowdps-model](model.md), [wowdps-core](core.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md): replay format 4 —
  `POOL_KINDS`, `MAX_POOLS`, `next_pools`, `fill_pools`; the cut's
  start-of-pull rows and time-sorted events; the decoder's pool bounds and
  tidy encoding; the rewrite queue recuts formats 1 to 3.
- **Update** — [wowdps-model](model.md), [wowdps-core](core.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md): R29's replay tier
  format 3 — `Post::absorb` and `Post::pools` (`set_pool`), the parser's
  `HpHint.more_power`, the buff rows and players' debuffs on NPCs, the
  `absorb` and `pools` sections; the rewrite queue recuts formats 1 and 2.
- **Update** — [wowdps-daemon](daemon.md): a `creatures.tsv` that exists
  but cannot be read is refused, at open and before a merge, rather than
  read as empty and rewritten.
- **Update** — [wowdps-daemon](daemon.md): `Store::merge_creatures` folds
  `creatures.tsv` back into its table before each merge, so an outside
  writer's rows survive the rewrite; a file refused at that re-read is
  never rewritten.
- **Update** — [wowdps-proto](proto.md), [wowdps-daemon](daemon.md),
  [wowdps-history](history.md): `creatures.tsv`, the addon's NPC
  classifications — proto's `creatures` codec and `addon_table`, the
  store's `merge_creatures` from the SavedVariables poll, `stats.creatures`.

## 2026-10-09

- **Update** — [wowdps-model](model.md), [wowdps-core](core.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md),
  [wowdps-history](history.md): R29's replay tier format 2 — every floor's
  posts (`Post::map_id`, `Cut::maps`), the boss rows, the `maps` section;
  the rewrite queue recuts format 1; `stats` counts the files by format.
- **Update** — [wowdps-gui](gui.md), [wowdps-core](core.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md), [wowdps-mcp](mcp.md),
  [wowdps-history](history.md): the architecture prose the agent instructions
  carried moves here — the GUI as built, surface by surface; core's modules
  and gates; proto's and the daemon's modules and the store's tiers; mcp's
  tools; history's views and subcommands — so the instructions (AGENTS.md
  per area) can stay rules. wowdps-encounter-rubric:
  the daemon reads its `placed` table since v45.
- **Update** — [wowdps-core](core.md), [wowdps-model](model.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md),
  [wowdps-history](history.md): the replay tier (R29, v45) — the cut, its
  rows, the `WDRP` codec and the CSV writer, the store's replay slots and
  `GetReplay`, `replay-export`; and the daemon's progression rule.
- **Update** — wowdps-encounter-rubric: the seasons
  and the schema README live inside the crate (`seasons/<season>/…`,
  `README.md`); the repository-root `rubric/` is gone, and every layer's
  paths stay `<season>/<instance>/<file>` relative to `seasons/`.
- **Update** — wowdps-encounter-rubric: an ordered layer
  stack in three tiers (`Tier::User` the default), `Source` / `Origin`,
  `with_curated`, `with_user`, `with_user_dir` and `user_errors`,
  `encounter_traced` with its `Provenance`, `layers_of` / `has_user`
  (its decision record left with it).
- **Creation** — wowdps-encounter-rubric: the seasonal
  encounter rubric brought to main from the replay branch under its new name
  (it was `wowdps-rubric` there), with the generated drafts, the instance and
  season files, and none of the hand-tuned curated files; `proto::dirs`
  came with it.
- **Update** — [wowdps-proto](proto.md): `dirs`, the XDG base directories
  resolved once (an empty or relative variable counts as unset);
  `talents::data_path` delegates to it.

## 2026-10-04

- **Update** — [wowdps-extract](extract.md): stub upgraded with its seams — tables
  by FileDataID, the unread hotfix cache, per-machine outputs, the planned
  replay generators ([decision](../decisions/replay-data-from-the-season.md)).

## 2026-10-03

- **Update** — [wowdps-history](history.md): `tests/duckdb_version.rs` holds the
  `duckdb` crate pin to the linked nixpkgs libduckdb, by name.
- **Update** — [wowdps-gui](gui.md) and [wowdps-gui-logic](gui-logic.md): named
  themes chosen in the ⚙ card, `[themes]` in config.toml, onyx and its faces
  ([decision](../decisions/themes-as-config-data.md)).

## 2026-10-01

- **Update** — [wowdps-gui](gui.md): the inspector widened over the stage and
  its graph's window scoping its lists ([decision](../decisions/a-window-scopes-the-drill.md)).
- **Update** — [wowdps-gui](gui.md): rewritten for the GPUI crate, which took
  this name at the cutover (merged as #71) — the `Session`, keys in
  contexts, one theme definition, the window's surfaces on their decisions,
  the overlay's edge strip, finite animation, the shot tests and the
  tolerance guard; the iced record's seams are retired with the iced crate.
- **Deprecation** — [wowdps-gui-new](gui-new.md): the GPUI GUI's build-time
  name; it became `crates/gui` (`wowdps-gui`), and its second package,
  wrapper, module option and CI steps went.
- **Update** — [wowdps-gui-logic](gui-logic.md): the GUI's framework-free
  half, drawn by one GUI since the cutover; the iced-only extension traits
  (`ColDraw`, `DensityPitch`) and key mapping no longer named.
- **Update** — [wowdps-daemon](daemon.md): `gui_binary` is unset in an
  ordinary config, kept so another build can stand in for the overlay.
- **Update** — [wowdps-gui-logic](gui-logic.md): wave B listed — the overlay's,
  the window's, the inspector's, the rail's, Home's, the palette's and the
  talent viewer's framework-free halves, moved from [wowdps-gui](gui.md).
- **Update** — [wowdps-gui-new](gui-new.md): no longer a skeleton — the
  overlay as an edge strip, the window over `Gui::fight`, keys in contexts,
  one scrollbar, and the shot tests that check its pixels against iced's.

## 2026-09-30

- **Creation** — [wowdps-gui-new](gui-new.md): the GPUI Kit GUI's skeleton
  (plan step 1.1): a `Session`, a window showing the daemon's `Status`, an
  empty `--overlay` layer surface, built per package.
- **Update** — [wowdps-gui-logic](gui-logic.md): `sibling::daemon_bin` moved
  in from [wowdps-gui](gui.md), which gui-new also starts daemons with.
- **Update** — [wowdps-daemon](daemon.md): config `gui_binary` names the GUI
  the overlay supervisor spawns (plan step 0.3); gains an Overlay supervisor
  section.
- **Update** — [wowdps-gui-logic](gui-logic.md): the bundled fonts moved in
  from [wowdps-gui](gui.md) (`fonts/`, `fonts::FONTS`), closing wave A.
- **Update** — [wowdps-gui-logic](gui-logic.md): the four art-cache readers
  moved in from [wowdps-gui](gui.md), generic over the GUI's image handle.
- **Update** — [wowdps-gui-logic](gui-logic.md): `keys` moved in from
  [wowdps-gui](gui.md) as a chord table; [wowdps-tui](tui.md)'s parity test
  iterates it (its stub gains Seams).
- **Update** — [wowdps-gui-logic](gui-logic.md): `history` (with `PAGE`)
  and the R26 `tree` moved in from [wowdps-gui](gui.md).
- **Update** — [wowdps-gui-logic](gui-logic.md): `hypr` moved in from
  [wowdps-gui](gui.md), its fake Hyprland under `test-support`.
- **Update** — [wowdps-gui-logic](gui-logic.md): `fold`, `simc` (with the
  paste store the talent viewer kept) and `single` moved in from
  [wowdps-gui](gui.md).
- **Update** — [wowdps-gui-logic](gui-logic.md): `config` moved in from
  [wowdps-gui](gui.md); the `test-support` feature carries its test hook.
- **Creation** — [wowdps-gui-logic](gui-logic.md): the GUIs' shared crate,
  opened with the theme's names and the table's column meanings moved out
  of [wowdps-gui](gui.md) (plan step 0.2, step 0).
- **Update** — [wowdps-gui](gui.md): its successor, gui-new on GPUI Kit with
  a shared `gui-logic`, is decided ([decision](../decisions/gui-on-gpui.md)).
- **Update** — [wowdps-gui](gui.md): the overlay's drill rolls abilities up into
  R26's groups (shut until pressed), through the window's tree lines.

## 2026-09-29

- **Update** — [wowdps-gui](gui.md): the inspector's ability list is the R26
  tree (`inspector/tree.rs`).

## 2026-09-28

- **Update** — [wowdps-gui](gui.md): the seams regrouped as the redesigned
  window stands — frame, stage, Home and the palette, chrome and type,
  review — with the design record and SHOTS.md under Source, linking the
  Redesign decision; `overlay.rs` described as edited only for the guard's
  seams and v35's test literals.
- **Update** — [wowdps-proto](proto.md): `ClientState::select_player`, the
  palette's opt-in, linking the Redesign decision.
- **Update** — [wowdps-gui](gui.md): the seams name the ribbon, the
  chronological Deaths table and "you" from `Row.mine`, linking the Wire
  decision.
- **Update** — [wowdps-proto](proto.md): `ClientState::raid` and the opt-in
  `open_death`, linking the Wire decision.
- **Update** — [wowdps-daemon](daemon.md): whose rows are the reader's
  (`mine.rs`, `HistoryLink::mine`), linking the Wire decision.
- **Update** — [wowdps-gui](gui.md): the seams name the top bar, the pull rail
  and the stored pull on the stage, linking the Rail decision.
- **Update** — [wowdps-proto](proto.md): `ClientState::log_id`, linking the
  Rail decision.
- **Update** — [wowdps-gui](gui.md): the seams name the inspector beside the
  meter, linking the Inspector decision.
- **Update** — [wowdps-proto](proto.md): a seams section for `ClientState`'s
  opt-in follow-selection, linking the Inspector decision.
- **Update** — [wowdps-gui](gui.md): the seams name the fight header and the
  filter in the tab row, linking the Header decision.

## 2026-09-27

- **Update** — [wowdps-gui](gui.md): the seams name the gold chrome, the
  window fonts and `theme::Look`, linking the Look decision.

## 2026-09-21

- **Update** — [wowdps-daemon](daemon.md): a history-store section, linking the
  retire-on-switch decision.
