---
type: Decision
title: Home Derives Client-Side From Fights
description: 'The GUI''s Home dashboard is derived in the client from `HistoryQuery::Fights` answers rather than from a new `Summary` query, and its list grows by scrolling rather than by a pager — with the daemon giving reads their own quota so a dashboard can never cost the user a stored fight.'
tags: [gui, history, wire]
status: stable
generated: { by: claude-code/opus-5, at: 2026-09-07T12:00:00-07:00 }
sources:
  - id: gui
    resource: ../crates/gui.md
    title: wowdps-gui — where Home lives
  - id: daemon
    resource: ../crates/daemon.md
    title: wowdps-daemon — the history thread whose queue this protects
---

**Where:** [`wowdps-gui`](../crates/gui.md) (`home.rs`),
[`wowdps-daemon`](../crates/daemon.md) (`history.rs`).

## Context

Home is a front door over the history store: pulls this season, keys, raid
progress, the owner's own numbers, the last day's fights. Every one of those
is already answerable from the cards `HistoryQuery::Fights` returns, and the
daemon also has purpose-built `Trend` / `Progression` queries that answer
some of them more directly.

Two hazards showed up in review before a line was written. `HistoryReq::Query`
rode the same bounded, deliberately lossy 64-slot channel as `HistoryReq::Store`,
so a dashboard polling the store while a pull ended could be the reason that
pull was never written. And `Fights`' `limit` came straight from the client,
while `wire::frame` only `debug_assert!`s on `MAX_FRAME` — a release daemon
asked for a whole lake would emit a frame its own reader rejects, which the
user experiences as a reconnect loop.

## Decision

Home derives everything from `Fights`, client-side. No `Summary` query is
added, no `Trend` call is made, and no wire message changes: the dashboard's
*shape* is the thing least likely to survive contact with use, and a query is
the expensive half to change. Promoting a panel into a daemon question is a
later, cheaper move once the shape settles.

Two daemon-internal fixes came first, both invisible on the wire:

- Reads (`Query` / `Fight`) may hold at most half the history queue. Past that
  a read is refused where a full queue would refuse it — counted, handed back,
  answered empty by the hub — so the slots a closing pull needs are always
  there. "Dropping beats stalling" stays true for writes only.
- `Store::fights` caps its page at `FIGHTS_CAP` after sorting. `total` stays
  unclamped so a reader still knows what it has not seen.

Paging is transport and never reaches the user: no page numbers, no next/prev,
no "load more". The list grows as the reader scrolls toward its end, with one
request in flight at a time — the client half of the same rule the quota
enforces — and stops asking once the cache holds `total`.

## Consequences

Home cannot show a Mythic+ rating, a raid's boss count, or a ladder
percentile, because no card carries any of them; those panels are absent
rather than approximated, and `unknowable_numbers_render_as_em_dash` exists to
keep them that way. Everything Home does show survives an offline daemon
restart, since it is the same JSON the store already holds.

The "near the bottom" test is computed from `absolute_offset`, `bounds` and
`content_bounds` rather than from iced's `Viewport::relative_offset`, which
divides by `content - viewport` and hands back a non-finite number while the
content is shorter than its viewport.
