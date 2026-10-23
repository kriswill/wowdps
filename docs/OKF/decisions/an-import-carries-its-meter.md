---
type: Decision
title: An Import Carries Its Meter
description: 'v48: an imported fight''s players carry their totals, reckoned by the importer under the meter''s rulings, and their 1 s damage and healing curves; the store derives the rates over the card''s combat clock and keeps the curves as the fight''s details tier, so every reader of a card or of that tier shows an import''s meter as it shows a local fight''s.'
tags: [history, replay, wire, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-19T18:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the imported fights paragraph (Daemon) and the v48 row of the version table
  - id: playbook
    resource: ../playbooks/bump-proto-version.md
    title: Bump PROTO_VERSION And Deploy It
---

**Where:** [`wowdps-proto`](../crates/proto.md) (`ImportedPlayer`'s
totals and curves, `ImportedCard::card`, `ImportedCard::details`,
`ImportedCard::check_curves`, PROTO_VERSION 48),
[`wowdps-daemon`](../crates/daemon.md) (`Store::import_replay` writes the
details tier), [`wowdps-mcp`](../crates/mcp.md) (`stored_fight` answers an
import's card) and [`wowdps-history`](../crates/history.md) (the lake's
`players` view reads the numbers as it reads a local card's). It amends
[Imported Fights Live In The History Store](imported-fights-in-the-store.md),
whose import was a card and a replay alone.

## Context

An imported fight (a Warcraft Logs fight, today) stored a card whose
players carried a guid, a name, a class and a spec, and its replay tier.
Every reader that draws a meter found zeros: the replay's followed-unit
HUD reads a player's damage curve from the details tier, the replay's
raid damage graph sums those curves, and the mcp and the lake read the
card's players. The source's events hold everything the meter counts, but
only the client that converted them has them, and the daemon is the
store's one writer.

## Decision

- **The importer reckons the totals, under the rulings.** Each
  `ImportedPlayer` gains the card's totals the source can honestly give:
  `damage` (R1, pets folded R5, no Augmentation share R19, no self-harm or
  friendly fire R22), `healing` with `overheal`, `absorbed` and
  `heal_absorbed` (R2, R3), `taken` (R17, Stagger's ticks out), `deaths`
  (R9), `support_given` / `support_received` (R19), `healed_received` /
  `self_healed`. What no import carries (a loadout, `logged`, R17's
  mitigation split, R18's spans, R20's ledger) stays unknown, and a 0
  means "none, or not known".[^contract]
- **No rate on the wire.** The store derives `dps`, `hps` and `dtps` with
  `wowdps_model::rate` over the card's rate clock, the model's one
  arithmetic, and an imported encounter's `combat_ms` is its duration
  (R7's START..END). An import ranks beside a local kill on equal terms,
  and nothing a client sends can disagree with its own totals.
- **The curves are the details tier.** Each player's 1 s damage and
  healing curves become `PlayerDetail::damage_timeline` / `heal_timeline`
  (`ImportedCard::details`), every list beside them empty, written through
  the same codec a local fight's details are. The replay (its `dps.tsv`
  and `raid.csv`) and `replay-export` read an import's tier as a local
  one's; the store's own answers do not — `stored_fight_in` answers a
  fight without a rows tier with its card alone (tier 1), the details
  unread. A
  curve must agree with its total (R12: a graph never desyncs from its
  total) and hold no second past the fight's end, or the import is refused.
- **Still no rows or series.** The rows tier is per-view meter rows,
  recaps and ledgers keyed per event, and the series tier is per-ability
  seconds; an import brings neither, and empty views would read as
  "nobody did that". A stored fight answers an import's card alone (tier
  1) as before; the mcp's `stored_fight`, which refused one, now answers
  that card, and refuses a drill or a zoom window, which have nothing
  under them, rather than answer the whole fight in their place.
- **Whole, like the rest of an import.** A re-import replaces the details
  with the card and the replay (one without curves takes them away), a
  failed write puts the old ones back, a moved import's old id keeps none,
  and the protected set keeps them from demotion.

## Consequences

The replay's HUD and raid damage graph, `replay-export`'s `raid.csv`, the
mcp's `stored_fight` and the lake's `players` view show an import's meter.
The GUI's history and home still ask for local fights alone, so they never
list one. An import's numbers are the importer's reading of another
tool's events, so they can drift from the game's own meter where the
source does (a split damage line it cannot tell apart, a pet it never
owned); the importer documents its mapping. Imports stored before v48 keep
their zeros until they are imported again. A wire-shape change: deploy per
the playbook, and restart every mcp server.[^playbook]

[^contract]: CONTRACT.md's imported fights paragraph (Daemon section) and its v48 row.
[^playbook]: The bump and deploy steps for a PROTO_VERSION change.
