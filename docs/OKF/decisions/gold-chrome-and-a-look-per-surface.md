---
type: Decision
title: Gold Chrome, Tabular Barlow, And A Look Per Surface
description: 'The window redesign''s first step dresses the WINDOW in the prototype''s tokens — gold chrome by default (the owner''s class as a config option), Barlow Semi Condensed with its tabular figures baked into the default digits, no semantic yellow — while every renderer the overlay shares keeps its old name and pixels and gains an `_in` twin that takes a surface''s `theme::Look`.'
tags: [gui, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-27T23:30:00-07:00 }
sources:
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — its Tokens section is the spec
  - id: fonts
    resource: ../../../crates/gui-logic/fonts/README.md
    title: crates/gui-logic/fonts — provenance and the tabular bake
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Design shots and the overlay snapshot guard
---

**Where:** [`wowdps-gui`](../crates/gui.md) (`theme.rs`, `line_icons.rs`,
`nav.rs`, `view.rs`, `compare.rs`, `timeline.rs`, `window.rs`) and
[`wowdps-gui-logic`](../crates/gui-logic.md) (`config.rs`, `fonts.rs` and
`crates/gui-logic/fonts/`, moved there for gui-new to share).

## Context

The prototype[^prototype] found the window's chrome loud and unstable: a
class-coloured gradient filled the headline card, the active tab and the
title; it started neutral blue and turned crimson only once Home resolved
an owner; and yellow meant five things (live, crit, stagger, the visit's Σ,
a wipe's best %). Its Tokens section fixes a palette (ground, surface,
raise, line, edge, three inks, gold, gold-dim, gold-ink, good, bad,
legendary) and one type voice for names and numbers.

Three constraints shaped how that landed:

- The overlay must not move a pixel. It shares a dozen renderers with the
  window — the recap rows, the ability drill's breadcrumb, stat strip and
  target list, the comparison, the drill graph, the instance strip — and
  the snapshot guard[^shots] hashes its pixels.
- No new crates: iced's "svg" feature (for the prototype's line icons)
  pulls some, and iced cannot switch an OpenType feature on.
- The chrome must be right on the first frame, before any owner is known.

## Decision

**Gold is the default chrome.** `theme::Chrome` is `Gold` or `Class`,
read from the config's `chrome` key (a plain string, like `density`, so a
typo reads gold rather than breaking the file). Gold needs nobody. `Class`
wears the owner's class — the character played last, as the store's
newest card names them — and the window writes that class
(`character_class`) whenever it learns it, so a class chrome is right on
the first frame too. (Config `character` is Home's scope alone since the
redesign's Home step, and locks nothing.) The accent is drawn only as an
underline under the active tab, a pressed chip's edge and a selection's
edge — never a fill. Class colour as text is `theme::class_text`, the
prototype's `textOn`: `Class::rgb` lifted toward white until it clears AA
on the surface; bars keep the raw colour. This refines
[One Chrome Accent](one-accent-from-the-class-color.md): its luminance
rule still derives the class chrome, but gold is what a window wears
unless asked.

**The window's type is bundled, and tabular by construction.** Barlow
Semi Condensed 400/500/600 and Marcellus (encounter titles only) are OFL
assets under `crates/gui-logic/fonts/` (moved there for gui-new), loaded by
`window::settings` alone.
Barlow's default digits are proportional, so the three files have its
`tnum` glyphs mapped over the default digits with fonttools, and the family
is renamed "Barlow Semi Condensed Tabular" so an installed proportional
copy can never be the face picked[^fonts]. `theme::tests::
the_window_digits_are_tabular` measures the digits through cosmic-text.

**A renderer the two surfaces share takes the surface's `Look`.** A
`theme::Look` names roles — ink, dim, label, the metric trio, good, bad,
live, Σ, crit, focus, the number face — and has two constants:
`Look::OVERLAY`, exactly the literals those renderers drew before, and
`Look::WINDOW`, the tokens. Each shared renderer keeps its name as the
overlay's (`recap_row`, `compare_body`, `timeline::strip`, …) and gains an
`_in` twin taking a `Look`; the window calls the twins. `overlay.rs` is not
edited at all, and the guard passes unchanged. Everything only the window
draws uses the tokens directly.

**The window's type scale is the prototype's.** `theme::size` holds the
Tokens specimens — encounter titles 27 (22 in a narrow window), stat values
16 at 500, names 15 and every figure 14.5, column heads and stat labels
13.5 in gold-dim, view tabs 14.5 and the top bar's places 15 — and
`theme::pitch` the row pitches (a meter row 32, a drill row 29, the total
33, a view tab 37, the top bar 44). No overlay code names either, so the
window's scale moves without an overlay pixel. The shared renderers keep
their literals (the overlay's); the window draws them at
`Look::type_scale` (1.2), which lands their rows near 14.5, and small
captions take `Look::caption`. Where only the layout knows the width —
the meter's columns under 800 px keep the amount and the rate, a drill
pane keeps what fits beside a readable name (`table::fit`), the title
drops to 22 — the window lays out through `responsive`. (Later steps moved
this: the Inspector step, `1864d9f`, took the window off the shared rows
the scale sized, and `type_scale` went with them, and made the meter's
two columns the layout's narrow, 820 px and under — see
[the Redesign decision](window-redesign.md).)

`Look` also carries the roles the window's rows need: the selected row's
fill (`RAISE`) and the pointer's (`--hover`), a recap hit's red, a graph's
plot (the surface), the strip's idle ring, and `live_dot` — the window
says a pull is live with a red dot beside its disc and never with a hue a
kill or a wipe wears. The overlay's values for each are what it drew
before.

The view tabs' emoji became `line_icons`, the prototype's 16-unit SVG
paths transcribed to canvas strokes; the top bar's gear and help and the
death chips wear them too.

**The window speaks the prototype's words.** Its views run in the
prototype's order (`view::WINDOW_VIEWS`: damage, healing, taken, deaths,
the counts, enemies) under their names ("Crowd control", "Enemies",
`view::window_view_name`); `View::ALL` and `fmt::view_name` stay the TUI's
and the overlay's. Every window string is sentence case — places, column
heads ("Amount", "Per sec", "Share"), stat labels, pane titles, badges
("Kill", drawn from the source's "KILL" by `nav::sentence`) — and a shared
renderer's fixed words capitalise only under `Look::capitals`. A view strip
too wide for the window hangs from whichever end holds its active tab
(`nav::anchored_at_end`), so the underline is in sight on the first frame;
`window::update` snaps it back to that end when the view changes, and a
plain wheel scrolls it (`Message::TabWheel`), which iced alone would not
do for a horizontal strip.

**Labels end in "…", not mid-glyph.** `ellipsis.rs` is the window's one
custom widget: a one-line label that keeps the longest prefix that fits
with the mark after it, measured by the renderer's own shaping, and found
by its whole text. It needs iced's `advanced` feature, which exposes iced's
widget traits and pulls no crate. The meter and drill rows, the comparison's
names, History's pulls and the recap wear it; the overlay's rows clip as
they did.

`Look` grew the roles the fixes needed, each the overlay's old behaviour in
`Look::OVERLAY`: `floor` (the smallest text a shared renderer may draw),
`fit` (a comparison table drops crit, then the average, before its names,
and seats its heads over its figures), `wipe_ring` (a wipe's disc is a
hollow ring, an outcome by shape as well as hue), `school_names` (the
window draws an ability's name in ink and keeps the school to its tag) and
`capitals`.

## Consequences

- Yellow is gone from the window as a semantic colour: LIVE is a red dot and
  its word, crit is ink, the Σ is secondary ink, a wipe's best % sits in
  secondary ink beside its WIPE. The overlay keeps all of its yellow.
- A new shared renderer, or a new colour in one, must go through `Look`, or
  the window inherits the overlay's palette (and the other way round would
  move the guard's hashes).
- Loading the window's fonts makes them process-global in a test run, which
  is why the guard must run alone (it checks, and names the fonts).
- The learned `character_class` is the one config write no gesture asks
  for, so it goes through `Config::store_character_class`: re-read the
  file, set that key, write it back — never the window's launch-time copy
  over an overlay drag. And only the owner's class is written (the
  character played last; an alt seen on the meter is worn for the session
  and never remembered). Home's scope (`character`) is written the same
  one-key way, `Config::store_character`, since a chip is a casual
  gesture. Every config
  save is atomic now (a sibling temporary renamed over the file, through a
  symlink to its target), and an EMPTY file — another writer caught
  mid-save by an older build — is never written back as the defaults.
- `hide_realms` now reaches every pane: `view::realmless` strips only what
  reads as a player's "Name-Realm-Region", because a drill's targets and a
  recap's sources mix players with creatures whose names hold hyphens.

[^prototype]: The window redesign prototype and critique, whose Tokens section is the palette and type this step implements.
[^fonts]: The fonts' provenance, the pinned google/fonts commits, and the reproducible tabular bake.
[^shots]: The headless design-shot harness and the overlay's pixel-hash snapshot guard.
