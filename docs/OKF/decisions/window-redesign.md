---
type: Decision
title: Redesign The Window From A Prototype, Reviewed In Headless Shots
description: 'The window redesign as a whole — gold chrome over class chrome, one pull rail over the log and the store, master and detail in place of a drill screen, Home as last night with a command palette over everything — built in six steps against an interactive prototype and reviewed at every step through headless renders of the real window rather than a launched GUI, the overlay held pixel-identical by a hash guard.'
tags: [gui, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-28T20:45:00-07:00 }
sources:
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — its critique ("What's wrong today"), build plan and Tokens
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Design shots, the chrome budget measure and the overlay snapshot guard
  - id: fonts
    resource: ../../../crates/gui-logic/fonts/README.md
    title: crates/gui-logic/fonts — provenance and the tabular bake
---

**Where:** [`wowdps-gui`](../crates/gui.md) — the window alone; opt-in
additions to [`wowdps-proto`](../crates/proto.md)'s `ClientState`; one wire
bump through [`wowdps-core`](../crates/core.md),
[`wowdps-daemon`](../crates/daemon.md) and [`wowdps-mcp`](../crates/mcp.md)
([R25](../rulings/r25.md)). Each step has its own record, linked below; this
one is the whole, and the step that has no record of its own (Home and the
palette).

## Context

The critique that opens the prototype[^prototype] was taken from the release
build against the live daemon, windowed at 1440×1000 on a headless output,
over one night's log and an 801-card history store. Its ten findings, ranked
by what each costs a reader after a pull: the meter's first row started 40 %
of the way down (14 of 25 raid rows in sight); one fight lived in two places,
with four controls that each moved over a different scope; drilling in threw
the meter away; the class-coloured accent was loud, unstable and said the
wrong thing; graphs had no axes and spans flooded them; deaths were sorted by
count; Home led with a vanity number; yellow meant five things and the reader
was not marked at all; the window spoke in two type voices and the system's
words; and a tail of smaller defects.

Four constraints bounded every answer:

- The overlay is in use mid-raid and shares renderers with the window; it
  must not move a pixel.
- The TUI shares `ClientState`; its semantics must not change.
- No new crates — so no iced "svg" — and stdlib everywhere but the frontends.
- The work was done while the game ran: no GUI could be launched beside it,
  and no release binary rebuilt (the dev unit bounces the daemon and its
  overlay on a release rebuild).

## Decision

**A prototype is the spec, and a picture of the real window is the review.**
`docs/design/window-redesign.html` is interactive — three widths (1440, 960,
460), both chromes, the keys — built from a real Heroic kill, with a six-step
build plan and a Tokens section; its renders at the three sizes are the
reference. Before any visual change, the first commit (`0c9f4e5`) built the
loop every later step was judged by[^shots]. `window::shots::design_shots`
draws the REAL window headless — iced_test's tiny-skia Simulator, the
window's own `view::view`, `window::settings()` and theme, each state reached
from a fresh window by the messages and keys a user sends, over the daemon's
mock parsing a real-log slice and a frozen copy of the history store — at the
prototype's three sizes, with a manifest that says whether two sets are
comparable. `overlay::guard::overlay_snapshot_guard` hashes the overlay's
pixels over the committed fixtures, so "the overlay did not move" is a test,
not a hope. Every step ended with its states rendered beside the reference,
and a finding that could be a number became one: the chrome budget (the first
row no more than 290 px down at 1440×900, 18 rows in sight) is measured on the
real log and held on every `cargo test` over a synthetic 25-player raid.

**Gold chrome over class chrome.** Gold is the interface, class colours are
people, green and red are outcomes, legendary orange is a personal best, and
nothing is semantic yellow. The owner's class survives as `chrome = "class"`,
resolved at start-up from the class the window last learned, so the first
frame is right either way. With it came the window's own type — Barlow Semi
Condensed with its tabular digits baked in, Marcellus for encounter titles,
OFL assets rather than dependencies[^fonts] — and a `theme::Look` per surface,
so a renderer both surfaces share keeps the overlay's literals under its old
name: [Gold Chrome, Tabular Barlow, And A Look Per Surface](gold-chrome-and-a-look-per-surface.md),
refining [One Chrome Accent, Split By Luminance](one-accent-from-the-class-color.md).

**Give the meter its height back.** One title line and one stat line, with
the owner's "you" chip, replace four stacked bands, and the filter moves into
the tab row: [One Fight Header Over The Meter](one-fight-header-over-the-meter.md).

**Master and detail.** The drill is an inspector beside the meter that
follows the selection through an opt-in mode in `ClientState`; spans leave
the curve for lanes coloured by their caster; a comparison overlays two
curves inside it: [An Inspector Beside The Meter](inspector-beside-the-meter.md).

**One pull rail.** Tonight's log and every stored night as one list,
deduplicated by fight id, and a stored pull opened in the same workspace
through a `ClientState` fed from `GetFight`: [One Pull Rail For Tonight And
Every Stored Night](one-pull-rail.md), which retires the History screen of
[One Table For Every Row List](one-table-for-every-row-list.md).

**One wire bump.** The ribbon's raid timeline, the chronological Deaths
view, the recap's time before death and a daemon-marked `mine` land together
as PROTO_VERSION 35, since every bump renames the socket and restarts every
client: [The Raid Timeline And Whose Rows On The Wire](raid-timeline-and-mine-on-the-wire.md).

**Home answers "how did last night go for me", and Ctrl K goes anywhere.**
Home still [derives client-side from `Fights`](home-derives-from-fights.md),
but reads a week rather than a growing list: one page in flight until the
seven days back from the store's newest card are in hand, `MAX_PAGES` at
most. It opens on the scope's newest night — a tile per pull with its outcome
and its place in the ROLE (effective dps, a healer's hps), and a rank slope
chart across the night — over three panels in a fixed order (keys against
par, raid progress with a dot per pull, effective dps on keys across
characters with each character's best ringed in legendary orange), each
worded apart when its week is empty, so the page keeps its shape. Scope is a
chip row, never a lock: config `character` is only the scope Home opens on,
while whose window it is, the chrome and "you" follow the character played
last — the lock that pinned an alt while the night was played on another is
gone. The command palette (Ctrl K, or the top bar's jump box) searches the
rail's pulls, the stage's players — selecting one by key through
`ClientState::select_player`, opt-in —, the views and the screens, folded as
the row filter folds; it is window-local, and its keys stay out of
`keys::action_for`.

**Six steps, each shipping something visible, and the overlay forked, never
changed.** Look, Header, Inspector, Rail, Wire, Home — the prototype's order,
none leaving the window half-converted. A window piece is a new window-only
module (`top_bar`, `fight_head`, `ribbon`, `deaths`, `inspector`, `rail`,
`palette`, `reveal`, `ellipsis`, `line_icons`); a renderer the overlay shares
takes a `Look`; `overlay.rs` changed only to give the guard its seams
(`settings()`, `aux_want()`, `mod guard`, all behaviour-neutral, in
`0c9f4e5`) and in test literals for v35's `raid` field, and the guard's
hashes were never re-blessed. Every
`ClientState` change is opt-in (`set_follow`, `open_death`, `select_player`)
or an additive accessor (`log_id`, `raid`, `segment_encounter`,
`view_answered`, `snapshot_gen`), so the TUI calls nothing new.

## Consequences

- The window's review loop is local and safe beside a running game: a debug
  `cargo test` renders every state at three sizes in about two minutes, and
  pictures of the real window, not descriptions, are what a design change is
  judged by. Its inputs (a real-log slice, a frozen store) and its shots hold
  real player names, so they live under `~/.local/share/wowdps/design-shots/`
  and never in the repository; two sets are compared only when their
  manifests agree on the inputs.
- The overlay guard proves pixels at iced_test's scale of 2 only, on the
  machine that blessed it (its fonts and icon caches are in the hashes), and
  must run alone: loading the window's bundled fonts is process-global and
  changes cosmic-text's fallback for the overlay's ⚙ Σ ☠ ● — the guard
  detects that and names the fonts.
- The window no longer uses most of what it shared with the overlay: the
  inspector, the fight header and the Deaths table replaced the drill
  screen, the comparison screen, the instance strip and the recap rows on
  the window's side, so several `_in` twins (`recap_row_in`,
  `compare_body_in`, the ability drill's pieces, `timeline::strip_in`) now
  draw for the overlay alone. Folding them back under the overlay's names
  would be a pixel-neutral simplification the guard can hold.
- The window reads as one system — a gold underline, class colours on
  people, red and green on outcomes — and the class chrome is still one
  config key away.
- Landed in `0c9f4e5` (the harness and the guard), `a617448` (Look),
  `91a5605` (Header), `1864d9f` (Inspector), `43e6005` (Rail), `74b00fb`
  (Wire) and `d7515da` (Home and the palette); the docs pass that closed it
  rewrote the GUI half of `CLAUDE.md` around this record.

[^prototype]: The window redesign prototype — its critique ("What's wrong today"), build plan and Tokens
[^shots]: Design shots, the chrome budget measure and the overlay snapshot guard
[^fonts]: crates/gui-logic/fonts — provenance and the tabular bake
