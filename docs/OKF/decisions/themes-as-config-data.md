---
type: Decision
title: Themes Are Data A Config Can Name, And Onyx Is The Second
description: 'The GUI''s one theme definition grows into a registry of named themes — navy (the old gold), onyx and frost — chosen in the window''s options card, every token group (colours, data hues, faces, sizes, corners, effects) a table config.toml can override on a built-in or use to define a theme of its own, with onyx the first theme to differ in faces, corners and effects as well as colour.'
tags: [gui, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-03T16:00:00-07:00 }
sources:
  - id: spec
    resource: ../../spec-gui-new.md
    title: gui-new specification — §6.1 Themes, its "As built" note
  - id: fonts
    resource: ../../../crates/gui-logic/fonts/README.md
    title: crates/gui-logic/fonts — Saira's and Michroma's provenance and the tabular bake
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: The headless review loop — WOWDPS_SHOTS_THEME and the render guard
---

**Where:** [`wowdps-gui-logic`](../crates/gui-logic.md) (`theme/`:
`tokens.rs`, `defs.rs`, `navy.rs`, `onyx.rs`, `frost.rs`, `registry.rs`;
`config.rs`'s `themes`; `fonts.rs` and `fonts/`) and
[`wowdps-gui`](../crates/gui.md) (`theme.rs`'s `Themes` global,
`window/w.rs`'s `r` / `pill` / `float`, `window/instruments.rs`,
`window/cards/options.rs`'s picker, `overlay/panel.rs`'s `take_theme`).

## Context

The GPUI rebuild kept one theme definition feeding Kit's `Theme` and the
app's `Look`, and left room for more[^spec]: `gold` (the redesign's
Tokens) and an unused `frost`. Three things were still not themeable:

- **Corners, faces and data hues were literals.** Eighty-odd `rounded(…)`
  calls named their radius; the graph marks, the stacked bands, the
  lettered squares and the foe's disc were constants in gui-logic.
- **Nothing a theme could add.** A theme could recolour, but not draw
  differently: glass, an instrument's marks.
- **No way to choose or extend.** `theme` was a config key nobody could set
  from the window, and a theme of one's own meant editing Rust.

The user asked for a theme picker, a monochrome sci-fi theme called Onyx
(watch dials and military instrumentation; class colours kept for bars and
charts; dark glass used sparingly; its own fonts), every part of the app
themed, and themes overridable — or written whole — in config.toml.

## Decision

**Every token group is a named table.** `theme::tokens!` writes a group's
struct and its field-name table together, so a field added to a group is
at once a config key, and a misspelt key is reported by name with a
suggestion. The groups: `window`, `overlay`, `talents`, `data` (the data
hues, moved out of `graph`, `inspect::stack`, `inspect::list` and
`inspect::lanes`), `faces`, `size`, `pitch`, `shape`, `bars` and `effects`.
`gold_dim` / `gold_ink` / `gold` became `label_ink` / `accent_ink` / `accent`
(the talent tree's `gold` became `taken`): a user writing a white theme
should not have to set a token called gold.

**The registry.** `theme::Registry::from_table` lays `[themes.<name>]`
over the built-ins: a table named for one overrides only what it says; any
other is a theme of the user's own, starting from its `base` (a built-in,
overridden or not, or another of their themes; `navy` when unsaid). Loops,
bad colours, unknown keys and non-tables are lines in `warnings` (stderr at
start, and the first in the ⚙ card), never a failed file. Built themes are
interned, so the overlay rebuilding the registry on every config change
leaks nothing. `wowdps-gui --print-theme <name>` prints every key of a
theme as tables to copy. The config keeps `themes` as written
(`toml::Table`), so a save never drops a key this version does not know.
Names: `gold` reads as `navy` for `theme`, and as `theme` for `chrome`.

**Corners scale, they are not enumerated.** `Shape { scale, chip }`:
every designed radius is multiplied by `scale` (`navy`'s 1.0 draws the
prototype's corners exactly), and a pill is capped at `chip` (`navy`'s 99
keeps pills; `onyx`'s 2 squares them). Discs stay round by meaning. One
number reproduced every existing radius, where a dozen named roles would
each have had to match the values they replaced.

**Effects are switches, off in navy.** `glass`, `brackets`, `dial`,
`fine_ticks`. With all four off and the scale at 1.0, `navy` draws the
pixels it drew before: the overlay render guard passed unchanged.

**Onyx.** A black-dial chronograph: true black ground, lume-white ink, a
neutral grey ramp, white chrome, the outcomes and the legendary orange
kept; class colours untouched as bars, discs and the sweep. Faces:
Saira Tabular — Saira's variable font (only it carries `tnum`)
instanced at width 80, where it sets Barlow's measure within a few per
cent: at its Semi Condensed width it ran 5–12 % wider, and the design
review caught the pull rail cutting names that fit in navy — and baked
tabular as Barlow is; Michroma, an extended face, for
titles, set smaller[^fonts]. Glass is only for what floats (cards, the
palette, the sheet, the toast, the tooltips, the overlay panel): the
`glass` fill at 98 % under a sheen, with a specular rim (a hairline child
on window cards — an inset shadow beside the drop shadow never drew) —
GPUI has no backdrop blur, and the design review found that any real
translucency (84 %, then 93 %) let the meter's words read sharp through
the cards: a defect, not glass. Instruments get instrument marks: reticle
brackets at the corners of the ribbon's and the inspector's plots (set
wide to the sides and close above and below, so they meet no label), the
ribbon's 10 s chapter ring, and — the one bold place — the inspector's
crest in a 60-tick bezel with the player's meter bar wrapped round it: the
sweep is their amount against the top row's, ending at a hand. The first
build swept their share of the view, and in a 25-player raid 4 % is a
sliver the review read as a stray mark; the bar's fraction reads at a
glance and agrees with the meter beside it. On black a faded class colour
turns to mud, so the `bars` group (how much class colour a bar shows) is
themed too, and onyx shows more. The user's review of the shots then
dropped the lust's name from the ribbon for the tooltip (in every theme: the
word either hid the crest or was struck out by it), restyled a pressed
action (`effects.quiet_press`: a raised key lit along its foot, where a
white-filled "Stop comparing" was the loudest thing on the stage), drew the
stack's rest as the player's whole curve (`Ink::StackRest`, graphite under
a `stack_other_edge` line), and removed the last baked colour, the class
chrome's `NEUTRAL` blue, for the theme's own accent.

## Consequences

- A new token is a field on its group and a value on every built-in; a
  new effect is a switch `navy` leaves off. Either way the Navy window
  shots and the overlay guard are the check that nothing moved.
- The overlay polls the config's mtime once a second to follow a switch;
  its own saves (a drag, a zoom) touch the file too, and `take_theme`
  compares definitions by value, so they repaint nothing.
- Data hues are themeable but validated: `stack_*` were chosen by the
  data-viz checks on navy's surface, and onyx keeps them.
- Layout constants (the overlay's literal sizes, a renderer's paddings)
  stay code: a theme changes the skin, never the breakpoints[^shots].

[^spec]: gui-new specification — §6.1 Themes, its "As built" note
[^fonts]: crates/gui-logic/fonts — Saira's and Michroma's provenance and the tabular bake
[^shots]: The headless review loop — WOWDPS_SHOTS_THEME and the render guard
