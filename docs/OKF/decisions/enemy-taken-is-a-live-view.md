---
type: Decision
title: Enemy Taken Is A Live View
description: R24's enemy damage-taken meter is a wire view (PROTO_VERSION 32) but never a stored one — a fight card stays about the group, so VIEW_KEYS stays a fixed seven.
tags: [meter, wire, history-store]
status: stable
generated: { by: claude-fable/5.1, at: 2026-09-14T20:26:04-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — rulings table row R24 and the wire version table row 32
---

**Where:** [R24 Enemy damage taken](../rulings/r24.md), [`wowdps-proto`](../crates/proto.md), [`wowdps-daemon`](../crates/daemon.md)'s history store.

## Context

An enemy damage-taken meter — one row per enemy name, the game's own
"Enemy Damage Taken" pane — needs a `View` so every frontend, the overlay's
click-cycle and the MCP tools can ask for it through the one `Watch` path.
Adding a `View` code is a wire-surface change (a v31 client would reject a
code-7 snapshot), and `View::COUNT`-sized tables would silently grow the
history store's per-view rows documents and the DuckDB `rows` view with it.

## Decision

`View::EnemyTaken` is code 7 and `PROTO_VERSION` moves to 32, renaming the
socket — the rename is the whole protection. The store is left out on
purpose: `proto::history::VIEW_KEYS` becomes a fixed seven-entry table
rather than `View::COUNT`-sized, so no card or rows document ever carries
enemy rows and a stored fight answers "no rows stored for this view". The
DuckDB views, the parity gate and every existing lake are untouched.[^contract]

## Consequences

The meter is live-only (and Overall-folded) where the game's is too; a
reader who wants a stored enemy picture has the Damage view's by-target
drill, which the rows tier already carries. If enemy rows are ever wanted
in the store, that is a schema decision of its own — a new key, a
`HISTORY_SCHEMA` bump and a DuckDB view — not a one-line widening of
`VIEW_KEYS`.

[^contract]: CONTRACT.md — rulings table row R24 and the wire version table row 32
