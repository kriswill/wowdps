---
type: Decision
title: One Fight Header Over The Meter
description: 'The window redesign''s Header step gives the meter its height back: one title line and one stat line (with the owner''s "you" chip) replace the title, the instance strip, the chip line and the stat cards; the filter moves into the view tabs'' row; the overkill column leaves the meter; and the total row follows a short list and pins under a long one.'
tags: [gui, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-28T12:00:00-07:00 }
sources:
  - id: prototype
    resource: ../../design/window-redesign.html
    title: The window redesign prototype — its `.fhead`, `.vtabs .filter` and `.ttotal`
  - id: shots
    resource: ../../../crates/gui/SHOTS.md
    title: Design shots, the chrome budget measure and the overlay snapshot guard
---

**Where:** [`wowdps-gui`](../crates/gui.md) (`fight_head.rs`, `view.rs`,
`nav.rs`, `reveal.rs`, `table.rs`, `ellipsis.rs`, `theme.rs`, `window.rs`);
an additive accessor in
`wowdps-proto`'s `ClientState` (`segment_encounter`). Part of
[the window redesign](window-redesign.md).

## Context

The prototype's first finding[^prototype]: above the meter's first row sat
the tab strip, a title line, the instance strip, a second title line with
`‹ ›` chips repeating the name and the verdict, two stat cards 110 px tall,
the view tabs, an always-on filter row and the column heads — about 380 px
of a 1000 px window, leaving a 25-player raid 14 rows. Its answer is one
title line and one stat line (`.fhead`), the filter folded into the tab row,
and the rows starting near 215 px.

Two things the prototype has the window does not: the live meter's snapshot
carries only its own view's rows (so the Damage view does not know how many
died, nor the Deaths view who survived), and no row says which player is
you (`Row.mine` is a later wire step).

## Decision

**One header, window-only.** `fight_head.rs` draws the title in Marcellus
(its line at the prototype's 1.1, so the title line is as tall as its
30 px step buttons) with its difficulty and size ("Heroic, 25 players",
from the snapshot's ENCOUNTER_START identity, whole at every width as the
prototype's narrow frame keeps it), the outcome badge (Live with
its red dot; Kill, Win, "Timed +2"; Wipe — "Wipe at 56%" when a health is
known — Loss, "Over time") and the duration, and the older / newer pull
buttons — `nav::icon_button`, the top bar's `.ibtn`, washed and lit to ink
under the pointer and faint and inert at the list's end: `‹` older and `›`
newer, as `←` `→` and `[` `]` step, with tooltips naming the keys. That
order is a departure: the prototype's `.fnav` puts newer first, and is left
as drawn — the window reads left as older because its keys do.
The title is what gives way: `Ellipsis::leaving` measures the meta's words
at layout (`nav::badge_extra` says what a badge adds) and the title ends in
"…" before a piece of the meta is pushed off. Under it, the stat line: the
view's raid figures as gold-dim label and ink value pairs, full figures
with commas — Damage's raid dps and damage, Healing's with the overheal
share, Taken's with what absorbs took, Enemies' "Raid dtps" and "Damage to
enemies" (the prototype's words), Deaths' count, a count view's count and
players — and the "you" chip: the owner's place among their own ROLE on
Damage ("17th of 19 dps 149,258"), the rate on Healing and Taken, the count
with its noun on a count view ("3 interrupts", "no interrupts" when they
have none), "died", "survived" or — while the pull goes on — "alive" on
Deaths; a press selects their row and scrolls the least that brings it
into view, clearing a filter that hides it — a chip made from what another
view said ("survived", "no interrupts") has no row to go to and takes no
press. The line is never shorter than the chip, so a view
without one (Enemies) lays its rows out where the rest do; the chip sits at
the line's end in a wide window, right after the pairs under 1180 px (a
tile), and wraps under them in a narrow one, where the header also takes
the prototype's narrow inset (`.fhead{padding:10px 12px 8px}`). Before
there is a fight the title line says "waiting for combat…" as a status, in
the quiet ink, and there is no stat line; while a view's answer is on its
way (`ClientState::view_answered`, or a stored pull loading) the stat line
is there, as tall as ever, and empty — never "Raid hps 0" from the last
view's rows. The instance strip,
the chip line and the stat cards leave the window's meter (the overlay
keeps its strip, and History and Home their cards).

**The owner is named by the config until the wire flags the row.** A row is
the owner's by the most certain thing that names it, asked of every row
before a less certain one is (`window::owner_among`, `Gui::owner_in`): the
guid of the character played last (as Home's answers name it — config
`character` is Home's scope, never who the reader is), then a
`history_characters` name or the name the
accent resolved, whole ("Name-Realm"), then a bare name by its name half
(as the daemon reads the key) — and a bare name only when it names one
row, so a namesake from another realm who out-ranks the owner never wears
their marks. The chrome's accent resolves by the same matcher. The owner's marks — the chip's
name, their row's "you" tag and rank — are in the owner's text colour
(`theme::you_text`, the prototype's `--you-text`), which stays AA on the
chip's class wash and the selected row's RAISE where a player's name colour
does not; the tag's frame and the chip's wash and edge are the raw class
colour.

**What other views said is kept for the fight it was said of.**
`fight_head::Seen` holds, for the watched segment only, the owner's row
from any view (never from a loading placeholder): what lets the Deaths
chip say "survived" and a count view's "no interrupts" when the owner has
no row there. It counts no deaths. A count taken from the Deaths view made
the Damage line change with the tabs the reader had visited, and a tick
between asking for Deaths and its reply could record a false "Deaths 0";
the Damage and Enemies lines name no deaths until the wire carries them.

**The filter is part of the tab row.** `nav::filter_box` is compact (a
search glyph, "Filter players" — "Filter enemies" on Enemies — and the `/`
keycap) at the end of the view tabs, 28 px tall, and widens with focus
(92 → 140 px of text), filled with the ground inside the floating edge, no
gold ring. The box IS the text field — the marks sit on an inert layer over
its insets — so a press anywhere lands on it and the release-to-focus and
`filter_visible` gating are unchanged. Its own keys reach the window
through iced: Enter is the field's submit (`FilterDone`: the text stays,
and iced's focus is dropped with the flag, or the next tick's poll would
take the keys again), and Escape, which a focused field captures for itself
and `keyboard::listen` never hears, is listened for captured while the
filter holds the keys (`FilterEscape`: the text and the keys given up).
The placeholder, like a roster's rank, is in `theme::INK_3_TEXT`, the faint
ink lifted to AA; the clear mark is a 24 px target.

**The active tab is always in sight.** The filter leaves a 460 px window's
strip about 280 px, and a strip anchored at one end by the active tab's
index showed a middle tab (Deaths, Interrupts) from neither end. A strip is
now seen through `reveal.rs`: at layout, whenever the active tab, the
strip's window onto it or the strip changed, it moves the least that brings
the active tab whole into sight; a wheel moves it along in between. An
edge with more of the strip past it fades into what the strip sits on
(`Reveal::fade`), so a tab cut there reads as going on rather than as a
stray glyph. The view strips' scrollable, their wheel message and the snap
on a view change are gone.

**The meter's columns are the view's.** `table::meter_set` gives the live
meter Amount, Per sec, Share and the view's fourth — Crit to a tenth
(`Col::CritFine`; a drill's crit stays whole), "Overheal" or Absorbed — and
a count view Count and Share; the parenthesised overkill leaves the meter
(a stored fight's table keeps `meter_cols`). "Amount" heads every Taken
table's amount now, the drill's and History's too, as the prototype's heads
do. The live meter stands on the prototype's own grid for its view
(`table::Grid::Meter`: `.v-num4` 68 / 66 / 52 / 58, `.v-enemy` 72 / 72 /
52 / 52, `.v-count` 72 / 72, 62 / 62 and a 26 px rank under 820 px, at a
12 px gap); every other table keeps `Col::width` and its 10 px gap. A sort
holds only while the view's table has its column (`Gui::meter_sort`):
Healing's overheal share is no order for Damage, and the choice comes back
with a view that has it. A sorted heading is gold with a line-icon arrow
after it (the window's faces carry no arrow glyph), and every sortable
heading lights gold under the pointer. `j` / `k` past the fold scroll the
list the least that shows the selection (`window::RevealSpan`). Tanks and
healers wear a shield or a cross after their name, which leaves room for
it. Figures at a billion and over read to two places
(`table::figure`: "1.49B", the prototype's `fC`) in every window table;
`human`, the overlay's too, is unchanged.

**The fight's workspace runs edge to edge.** The meter and the comparison
leave the window's 10 px frame (`.stage`): the tabs' and headings'
hairlines, the selected row's wash and the total span the window, each
piece in the prototype's own inset (the header 18 at the sides, the tabs
`0 12 0 10`, the rows a lead that puts the rank's end at 34 and the disc at
45); "Player" and the total's label start over the icon column, as the
prototype's `.who2` head does. A drill and the comparison's body keep the
frame inside it.

**The total row follows or pins by measure.** iced has no sticky
positioning, so the table measures itself (`responsive`): a list that fits
takes its own height with the total straight under its last row; a longer
one scrolls above a total pinned to the bottom. The live meter's total is
the prototype's `.ttotal`: a surface under a hairline, the amount and the
rate, the share "100%" and the fourth figure blank; under a filter it is
the drawn rows' — "Total, 3 players", their sum — with no share
(`table::Fold::Meter`), its figures at 400 and its label at the frame's
14 px, as `.ttotal`'s are. Every other total folds every column
(`Fold::Full`). The footer appears only when the daemon's status says
something.

## Consequences

- At 1440×900 over the Coiled Altar kill the first row starts 199.6 px
  down and 20 rows of 25 show; a synthetic 25-player raid holds the same
  budget in `cargo test`, and the real log is measured by an ignored test
  beside the design shots[^shots].
- The overlay's snapshot guard passes unchanged. Renderers it shares were
  extended with overlay-neutral defaults rather than changed: `bar_row`
  gained a tagged twin whose `None` tags path is the old row, `Ellipsis` a
  `leaving` and a line height it defaults off, and `nav::tab_strip` (which
  the overlay never draws) its reveal.
- Follow-ups the Wire step owns, since they wait for the raid's marks on
  the wire: the Deaths pair on the Damage and Enemies lines ("Deaths 6"),
  Deaths' "First m:ss" and "Battle rezzes", the time in the chip's "died
  m:ss", and a live pull's best health for "Wipe at N%" (`Verdict::wipe_pct`
  is worded but nothing on the live meter fills it). An ignored test,
  `the_stat_line_names_the_deaths_once_the_wire_carries_them`, holds the
  stat line's half. When they land, and `Row.mine` with them, `Seen` can
  go.

[^prototype]: The window redesign prototype and critique — finding 1, and the `.fhead`, `.vtabs .filter` and `.ttotal` rules.
[^shots]: The headless design-shot harness, the chrome budget measure and the overlay's pixel-hash snapshot guard.
