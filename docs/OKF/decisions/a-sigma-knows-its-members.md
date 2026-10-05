---
type: Decision
title: A Σ Knows Only What Its Members Knew
description: 'A visit''s Overall starts with no identity of its own and learns owners, names, classes and the rest from its members alone, and a closed segment learns nothing more, because seeding the Σ from the meter made a full replay credit a guardian owned only after a key''s END that no member and no lazy load credited.'
tags: [meter, ruling]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-04T23:50:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R10 (Visits & Overall), the Overall bullet
---

**Where:** [`wowdps-core`](../crates/core.md) (`Segment::empty` beside
`Segment::new`, `Meter::overall`, and the open-segment guard on `learn`,
`note_summon`, `note_owner` and COMBATANT_INFO's class, spec and loadout),
[R10](../rulings/r10.md); gated by `crates/core/tests/instance.rs`.

## Context

A `/code-review` real-log check found two keyed Σs that loaded lazily short of
a full replay: Den of Nalorakk +13 by 65,856 damage and The Blinding Vale +11
by 65,053, on `main`. Every member matched; only the Σ differed. `Meter::overall`
built the Σ through `Segment::new`, which seeds a segment's identity maps
(owners, names, flags, classes, specs, loadouts, summons) from the meter as it
stands, so a segment opening mid-log still knows an earlier pet. For a Σ that
"now" is when it is read: the end of the file in a full replay, the end of the
visit in a lazy load.[^contract] A Lightspawn Lasher's damage lines name only
their victim; the first line naming its owner is its "Sappy Demise" cast, 28
lines after the key's END. The full replay's Σ folded the Lasher's ten hits onto
its owner; the boss member that holds them, and every lazy load, did not. The
daemon's live Σ gained the 65k a fraction of a second after the END.

A neighbouring leak: `learn`, `note_summon`, `note_owner` and COMBATANT_INFO
wrote into the newest segment even after it had closed, so a statement after a
key's END (or COMBATANT_INFO lines right after a START closed the trash) landed
in a closed member its lazy load never sees. R8's `infer` already refused a
closed segment for exactly this reason.

## Decision

The Σ starts with no identity (`Segment::empty`) and learns what its members
knew through `absorb`'s union; `Segment::new` is `empty` plus the meter's
seeding. The five identity writers update the newest segment only while it is
open. Lazy = full by construction again, and a full replay's Σ is its members
merged and nothing more. The scanner needed no change: its byte ranges already
made each segment its own slice.

## Consequences

Both real Σs match on Damage, Healing, Taken and Deaths. The trade-off is
recall: a guardian whose owner is named only after the visit is now uncredited
in the Σ, as the member holding its hits already left it uncredited. Σ cards
change by those amounts when regraded. The test replays a key whose guardian's
owner is named after the END, before and after post-key combat; each half of
the fix fails one variant when removed.

[^contract]: CONTRACT.md — R10 (Visits & Overall), the Overall bullet
