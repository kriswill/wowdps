---
type: Decision
title: One Chrome Accent, Split By Luminance
description: 'The GUI''s accent is derived from the class color, and the light/dark split that picks its ink is WCAG''s crossover luminance (0.179) rather than a tuned constant, because the split exists to keep text legible on thirteen fixed colors nobody gets to choose.'
tags: [gui, design]
status: stable
generated: { by: claude-code/opus-5, at: 2026-09-07T12:00:00-07:00 }
sources:
  - id: gui
    resource: ../crates/gui.md
    title: wowdps-gui — theme.rs
---

**Where:** [`wowdps-gui`](../crates/gui.md) (`theme.rs`).

## Context

The window's chrome takes its color from whoever the meter is about, so a
Priest's window and a Death Knight's are told apart at a glance. The class
colors are Blizzard's and cannot be adjusted: they run from Priest white to
Death Knight's dark red. Anything drawn ON the accent — an active tab's label,
a headline stat — therefore needs ink chosen per class, and the design study
proposed a 0.6 relative-luminance threshold with Warrior tan named as the
calibration case that must land on the light side.

Those two statements cannot both hold. Warrior (0xC69B6D) has a relative
luminance of ≈ 0.40, well under 0.6, and near-white ink on it contrasts at
2.1:1 — unreadable.

## Decision

The threshold is WCAG's own crossover, 0.179: the luminance at which black
text and white text contrast equally. Above it the accent takes near-black
ink and its second gradient stop darkens; below it, near-white ink and the
stop lightens. Every class then clears 3.5:1 by construction, which
`every_class_is_legible` asserts over all thirteen.

Warrior, Priest, Rogue and Monk land light as the study wanted. Druid,
Warlock and Evoker also land light, where the study's prose put them dark —
the arithmetic disagrees with the prose, and the contrast test is what a
future tweak has to satisfy, not the list.

## Consequences

`Accent` carries `base`, `lift`, `ink`, `heading` and `light`, and is the only
place a class color becomes chrome. `spec` is already a parameter and is
ignored, so a within-class tint later is a one-function change. The palette
that used to live in `view.rs` moved here and is re-exported from `view`, so
the overlay, compare, talents, timeline and gauge renderers were untouched.
