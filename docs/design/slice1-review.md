# Slice 1 — visual review

Taken from the real window, tiled at 1396x1074 on DP-1 workspace 1, connected
to the live daemon on a real raid log (The Coiled Altar, 21 players) and a
store holding 462 cards. This is what the eye caught; `slice1-qa.md` covers
correctness separately.

## Blocking the look

1. **Home ignores the width it has.** Every panel is a full-width single
   column, so on a 1396 px window each row is a name on the far left and a
   number on the far right with a hand's breadth of nothing between them. The
   design calls for a responsive grid (auto-fit, ~15rem minimum), which at
   this width is three columns and at 460 px is one. Fix the container, not
   the panels — they are already the right shape.

2. **Metric text drowns in its own bar on the meter.** Rows whose bar reaches
   the number columns (ranks 3-8 in the capture, the mid-luminance greens and
   olives especially) render dps and % as dim grey over a lit gradient, at
   roughly the contrast of a watermark. `view.rs` already has
   `inverted_metrics` for exactly this, and it is either not consulted on the
   gradient path or its threshold was tuned for the old flat fill. The
   gradient's bright end is what the numbers sit over, so the test must use
   the END colour, not the base.

## Missing from the design, not yet built

3. **No jump-chip row on Home** (Season / Keys / Raid / Me / Characters /
   Recent). The tab bar landed; its second row did not.

4. **No season-progress panel** — the per-role bars with the weekly curve.
   The "me" panel covers part of it, but the by-role breakdown is absent.

5. **No sparklines** anywhere. The "me" panel is three numbers where the
   design has three numbers and a twelve-point curve.

## Wording and detail

6. **The characters panel reads oddly**: "jump to Tranqlock-Proudmoore-US
   · 188". It is a single row with a verb where the design has a list of
   characters with class colour and a count. With one character in the store
   the panel should either name it plainly or say that alts appear here once
   the store sees them.

7. **The raid panel mixes two row shapes** — most bosses show a kill time on
   the right, one shows "best 81% · 23 pulls". That is correct behaviour
   (kill vs no kill) but it reads as a formatting bug; give the no-kill case
   the same visual weight and column position as a time.

8. **The footer still carries the long hint string** on the meter. The `?`
   sheet exists now, which is what earns the right to shorten it to two or
   three contextual hints.

## Confirmed working

- The tab bar, with History correctly disabled and dimmed.
- The two-tone title, the spec accent, the gradient bars, the stat cards
  including the hero-gradient card.
- `~` opens Home over a live fight and Esc returns; the meter keeps running
  underneath.
- The filter box renders with its `/` affordance.
- Home paged the whole store by scrolling — "462 of 462" — with no pager
  control anywhere, exactly as decisions §9 requires.
- Real numbers throughout: 455 pulls, 154 kills, 119 this week, per-dungeon
  key levels with timed counts, "5 down · 6 seen" rather than an invented
  denominator, and `best 81%` from `best_pct`.
