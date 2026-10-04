# Log

## 2026-10-03

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
