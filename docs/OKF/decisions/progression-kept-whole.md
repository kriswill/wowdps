---
type: Decision
title: A Progression Wipe Is Kept Whole Until The First Kill
description: 'v45: a wipe on a boss the store has not seen killed at that difficulty is progression, kept whole as a kill is — details, series and replay, protected from the caps, whatever its length — so analytics and the coach have every pull of a progression; the first kill there returns those wipes to the caps.'
tags: [history, retention, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-09T15:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the daemon's history store and retention, the v45 row
  - id: spec
    resource: ../../spec-history-store.md
    title: The history store specification — §6 what is stored, §7 retention
---

**Where:** [`wowdps-daemon`](../crates/daemon.md)'s history store
(`Store::is_progression`, `earns_details`, `earns_series`, `kept_whole`, the
protected set, `kept_counts`; config `history_keep_progression_whole`),
[`wowdps-proto`](../crates/proto.md)'s `HistoryStatus` (`kept_kills`,
`kept_keys`, `kept_progression`, `kept_pins`) and
[`wowdps-history`](../crates/history.md)'s `stats.kept`.

## Context

v42 kept every boss kill and timed key whole for the season
([Stored Pulls Kept Whole](stored-pulls-kept-whole.md)), but the wipes
before a first kill — the pulls a progression report compares fight over
fight — still answered to the per-group caps: past ten with details the
oldest lost them, a wipe under a minute never had any, and none kept the
series or (now) the replay tier.[^spec]

## Decision

"Unkilled" is per (encounter id, difficulty id): no stored card has
`success == Some(true)` there — a Heroic kill says nothing of Mythic wipes.
Such a wipe (not aborted) is PROGRESSION: `earns_details`, `earns_series`
and the replay slots all answer yes, and it joins the protected set, so the
caps neither demote nor evict it. Once the first kill at that difficulty is
stored, the next retention pass (every write runs one) finds the wipes no
longer progression: they answer to the caps again — pins still protect —
and a wipe after the kill is an ordinary wipe. Kills (every one) and timed
keys stay kept whole as before. `history_keep_progression_whole` (on by
default) turns it off. The rewrite queue backfills what the rule now
promises progression wipes already in the store; `HistoryStatus` and
`wowdps history stats` count the kept-whole set by why.[^contract]

## Consequences

On the machine this was built on, 47 of 705 boss cards were progression
wipes (4 993 s of fight): about 25 MB more (details at ~2.0 KB per fight
second, series ~1.7, replay ~2.2) for every pull of every unkilled boss.
The set shrinks by itself at each first kill.

[^contract]: CONTRACT.md — the daemon's history store and retention, the v45 row
[^spec]: The history store specification — §6 what is stored, §7 retention
