---
type: Decision
title: Keep A Keystone Run's Affixes
description: 'v47 (R10, replay format 6): the affix ids a CHALLENGE_MODE_START lists are parsed onto the keyed visit and kept on the run''s card, its replay head and every reader of them, landed ahead of the rest of key-level scaling because the raw logs that can backfill every stored key survive only for now.'
tags: [history, replay, wire, parser, keystone]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-17T12:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R10's affix paragraph, R29's TIER FORMAT 6 and the v47 row of the version table
  - id: playbook
    resource: ../playbooks/bump-proto-version.md
    title: Bump PROTO_VERSION And Deploy It
---

**Where:** [`wowdps-core`](../crates/core.md) (`Event::ChallengeModeStart`,
`meter::Visit::affixes`, the replay cut's head), [`wowdps-model`](../crates/model.md)
(`replay::KeyHead::affixes`), [`wowdps-proto`](../crates/proto.md)
(`history::KeyInfo::affixes`, the wire's v47 card key, replay format 6,
`pull.txt`), [`wowdps-daemon`](../crates/daemon.md) (the card writer, the
rewrite queue), [`wowdps-mcp`](../crates/mcp.md) (`instance.affixes`) and
[`wowdps-history`](../crates/history.md) (`fights.affixes`). The rulings:
[R10](../rulings/r10.md) and [R29](../rulings/r29.md).

## Context

A keystone run's affixes change what its numbers mean: the same dungeon at
the same level hits harder or lasts longer on another week's set, and the
key-level scaling the Mythic+ plan wants (a reading normalized to a chosen
level, a rubric number read at any level) needs them. The combat log names
them in one place, the trailing bracket of `CHALLENGE_MODE_START`
(`CHALLENGE_MODE_START,"Name",mapID,challengeID,level,[10,9,147]`), and the
parser dropped it, so the store kept the level alone.

The store keeps summaries, not logs. A stored fight is refilled from its
log by header hash: a regrade rewrites its card, and the rewrite queue
recuts a replay older than this build's format. Both need the log, and the
logs covering every stored key so far (about 69 files, 44 GB, August to
October 2026) are still on this machine. They will not be forever. The
display and the scaling rules can wait; the data cannot, so this half lands
first, on its own.

## Decision

- **Parsed where the log says it.** `Event::ChallengeModeStart` carries
  `affixes: Vec<u32>`, read from the comma split's `[10`, `9`, `147]` up to
  the field that closes the list, so a field a later patch appends after it
  is left alone. `[]`, a list that never closes and an item that is no u32
  read as none: the parser never fails a line.
- **On the visit, not the scanner.** The keyed visit the START opens keeps
  them (`Visit::affixes`). They move no number, clock or verdict, so the
  index's scan keeps no copy and its checkpoint format stands; a lazily
  loaded Σ reads them off the START, its own first line, and a pull inside
  the run off its seeds. A key joined mid-run never saw its START and keeps
  none.[^contract]
- **On the card, written only when there are some.** `KeyInfo::affixes`
  writes `"affixes": [...]` last in the card's `key`, and only for a key
  that names some, so every card without them is byte for byte what it
  was, the way v46 wrote `origin`. On the wire the card's key carries the
  list always (it is embedded), which is the v47 bump. The live snapshot
  names no key (a Σ's level is in its name), so no live frame moved.
- **In the replay head, as format 6.** `KeyHead::affixes` follows the level
  in the head (a count, then each id); `pull.txt` says `affixes = …`. A
  format number names what the cut wrote, so the bump is what makes the
  rewrite queue recut every older file from its log.
- **Readers expose them, nothing reads them yet.** The mcp card's
  `instance.affixes` and the lake's `fights.affixes` (probe-typed, NULL off
  a key, for a key joined mid-run and on an older card). No grade,
  comparison or rubric uses them; that is the scaling step's design.

## Consequences

- **The backfill is two steps after deploy, both from the logs.** The
  rewrite queue recuts every stored replay in a replay slot to format 6
  while idle, with no command; the cards need `wowdps history regrade
  --kind key` (a recut writes the replay alone, never the card). Both work
  only while the run's log survives; a key whose log is gone keeps a card
  and replay without affixes, read as none.
- **Every older replay is recut, keyless ones too.** Format 6 changes only
  a head that names a key, so a raid pull's format-5 file is format 6's
  bytes but for its number. The queue recuts it anyway rather than
  re-stamp it: the queue knows a file by its number alone, a recut reads
  only the fight's own lines (its log's index comes from the cache), and
  the last bump, format 4 to 5 over 595 files, took about three minutes of
  idle time. A re-stamp would be a second write path into the tier to save
  that once; the churn is accepted.
- **An import must be cut again.** The store takes an import in this
  build's replay format only, so a client that converts fights is rebuilt
  from this tree.
- **Deployment follows the playbook:** the socket is `wowdps-v47.sock`, and
  every client, each session's mcp server included, restarts onto it.[^playbook]

[^contract]: CONTRACT.md — R10's affix paragraph, R29's TIER FORMAT 6 and the v47 row of the version table.
[^playbook]: docs/OKF/playbooks/bump-proto-version.md.
