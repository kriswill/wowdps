# Log

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
  per area) can stay rules. [wowdps-encounter-rubric](encounter-rubric.md):
  the daemon reads its `placed` table since v45.
- **Update** — [wowdps-core](core.md), [wowdps-model](model.md),
  [wowdps-proto](proto.md), [wowdps-daemon](daemon.md),
  [wowdps-history](history.md): the replay tier (R29, v45) — the cut, its
  rows, the `WDRP` codec and the CSV writer, the store's replay slots and
  `GetReplay`, `replay-export`; and the daemon's progression rule.
- **Update** — [wowdps-encounter-rubric](encounter-rubric.md): the seasons
  and the schema README live inside the crate (`seasons/<season>/…`,
  `README.md`); the repository-root `rubric/` is gone, and every layer's
  paths stay `<season>/<instance>/<file>` relative to `seasons/`.
- **Update** — [wowdps-encounter-rubric](encounter-rubric.md): an ordered layer
  stack in three tiers (`Tier::User` the default), `Source` / `Origin`,
  `with_curated`, `with_user`, `with_user_dir` and `user_errors`,
  `encounter_traced` with its `Provenance`, `layers_of` / `has_user`
  ([decision](../decisions/encounter-rubric-layers.md)).
- **Creation** — [wowdps-encounter-rubric](encounter-rubric.md): the seasonal
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
