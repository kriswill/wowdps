# Log

## 2026-09-30

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
