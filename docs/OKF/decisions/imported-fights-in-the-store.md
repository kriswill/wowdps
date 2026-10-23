---
type: Decision
title: Imported Fights Live In The History Store, Apart From Ours
description: 'v46: a fight converted from outside this machine''s logs (a Warcraft Logs fight, today) is handed to the daemon whole and stored as a card that names its origin plus its replay tier and nothing else, protected like a pin, listed only to a client that asks, and never read by the store''s judgments of this machine''s history — so the replay can read the store alone while the daemon stays its only writer.'
tags: [history, replay, wire, design]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-11T16:30:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — the imported fights paragraph (Daemon) and the v46 row of the version table
  - id: playbook
    resource: ../playbooks/bump-proto-version.md
    title: Bump PROTO_VERSION And Deploy It
---

**Where:** [`wowdps-proto`](../crates/proto.md) (`history::Origin`,
`ImportedCard`, `imported_id`; `ClientMsg::ImportReplay`, the `History`
answers `ImportedReplay` and `Refused`, `OriginFilter`, PROTO_VERSION 46),
[`wowdps-daemon`](../crates/daemon.md) (`Store::import_replay`, the
protected set, the judgments that skip imports), and
[`wowdps-history`](../crates/history.md) (the lake's `imported` column).
It builds on [The Replay Tier Lives In The History Store](replay-tier-in-the-store.md).

## Context

The replay drew a fight converted from Warcraft Logs from a pull directory
the extractor wrote, beside the store's own replays. Two sources meant two
readers and a directory of files nothing kept or trimmed. The store already
held a replay tier for every pull this machine logged, so the converted
fight belonged there too, but the store's records had all been
derivable from this machine's logs: the card, the rows, the details and the
series are summaries the `Meter` recomputes, and the owner, progression and
personal bests are read off them. A converted fight has no log here, and is
usually somebody else's.

## Decision

- **Store-only, through the daemon.** A client converts the fight and sends
  `ImportReplay`: the facts its source knows (an `ImportedCard`) and the
  replay tier's bytes. The history thread is the only writer: it validates
  the replay (this build's format, a whole decode) and the facts (a source,
  a boss pull with its encounter), stores the card and keeps the replay
  through `write_replay` — the log path's `store_replay` without its trim
  — the size cap trimming only once the card lands. No client writes the
  store's files.[^contract]
- **This build's format alone.** A replay format number names the cut's
  rules, not only the codec (formats 4 and 5 moved with the rules on an
  unchanged codec), and the rewrite queue brings a local replay forward by
  cutting it again from its log. An import has no log, so a file in an
  older format, kept as it is or re-stamped, would claim rules it was not
  cut by: it is refused, and the client (`wcl-pull --store`, built from the
  same tree) cuts it again.
- **A write that fails changes nothing.** A re-import writes its replay
  before its card; when the card's write fails, the old replay goes back
  under the old card. A busy queue answers `Refused` and is no write lost,
  so `HistoryStatus::dropped`, which a client warns about, does not tick.
- **A card that names its origin.** `FightCard.origin` is `Local` or
  `Imported { source }`, written in the card's JSON only for an import, so
  every local document stays byte for byte what it was. The id is
  `imported_id(source, start)`: `fight_id`'s shape with the hash taken over
  `import:` and the source, never a log's first line; an id a local card
  holds is refused all the same. The same source again replaces the card
  and the replay and says so.
- **The replay tier alone.** No rows, details or series: the store derives
  those from log lines through the meter's rulings, and a converted fight's
  events are another tool's reading of a log, with its own amounts. A
  stored fight answers the card alone (tier 1); the GUI's stored pull and
  the mcp's `stored_fight` already handle a card without rows. (Amended in
  v48 by [An Import Carries Its Meter](an-import-carries-its-meter.md): the
  importer reckons each player's totals under the rulings and sends their
  1 s curves, which the store keeps on the card and as the details tier;
  still no rows or series.)
- **Protected, like a pin.** An import is a deliberate act, and the store
  cannot make it again: there is no log to rewrite or recut it from. So it
  is never evicted, demoted or trimmed, by the caps or by
  `history_replay_mb`, and `Status` counts it apart (`kept_imports`).
- **Not this machine's history.** Every judgment the store makes of its
  owner's history skips imports: the owner inference, the account's
  characters (`mine`), a boss's first kill (an imported kill ends no
  progression of ours), the fastest kill and the owner's bests;
  `Progression`, `Trend` and `RoleNight` count local fights alone, and the
  lake's fixed questions and its `role_ranks` grader do the same
  (`NOT imported`), so parity holds.
- **Listed on request.** `HistoryQuery::Fights` carries an `OriginFilter`,
  and every client that predates imports asks `Local`: the GUI's history
  and home and the mcp read exactly what they read before (the TUI asks
  the store nothing). A client that wants imports (the replay, next) asks
  `Imported` or `Any`; a listed import that names no owner stays ownerless,
  never stamped "me".

## Consequences

The replay can drop its pull-directory source and read the store alone,
and a converted fight is kept, trimmed by nothing, beside this machine's
own. Imports count against `history_replay_mb`'s total without ever being
trimmed by it, as pins do, so many large imports squeeze out unprotected
local replays first. An imported card carried no numbers, so a client that
lists one drew its name, date, verdict and roster, and its replay; since
v48 it carries its players' meter too. A
wire-shape change: deploy per the playbook.[^playbook] Landed in PR #108
(the wire and the record, the store, the lake's column, and the fixes an
independent review asked for).

[^contract]: CONTRACT.md's imported fights paragraph (Daemon section) and its v46 row.
[^playbook]: The bump and deploy steps for a PROTO_VERSION change.
