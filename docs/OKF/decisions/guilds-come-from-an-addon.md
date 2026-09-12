---
type: Decision
title: Guilds Come From An Addon, Joined At Read
description: 'The combat log never names a guild, so wowdps ships a tiny in-game addon that writes every raid member''s guild to its SavedVariables; the daemon files those as affiliations and joins them onto cards when it answers, never storing a guild on a card, and only ever updates an addon the user installed.'
tags: [daemon, history, proto, wire, addon]
status: stable
generated: { by: claude-code/fable-5.1, at: 2026-09-09T12:00:00-07:00 }
sources:
  - id: daemon
    resource: ../crates/daemon.md
    title: wowdps-daemon — addon.rs and the history thread
  - id: proto
    resource: ../crates/proto.md
    title: wowdps-proto — lua.rs and the Affiliation record
  - id: spec
    resource: ../../spec-history-store.md
    title: History store spec §9a
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — wire v31
---

**Where:** [`wowdps-daemon`](../crates/daemon.md) (`addon.rs`, `history.rs`),
[`wowdps-proto`](../crates/proto.md) (`lua.rs`, `history::Affiliation`),
`addon/` at the repository root, wire v31[^contract], spec §9a[^spec].

## Context

Nothing in a combat log says which guild a player is in — not the player
line, not `COMBATANT_INFO`, not any system message — and a sweep of every
Blizzard-authored `SavedVariables` file on a real install found no guild
either (`Blizzard_Communities` stores club ids, nothing named). Third-party
addons do record it, but only for the account's own characters. The
history store wanted the guild of every raid member to tell a guild night
from a PUG and to name "me" on a lake with one log.

The only honest source is the game's own API from inside the game, and an
addon cannot write a file mid-session: SavedVariables are flushed on
logout, `/reload` or exit. So whatever is built learns about a night after
it.

## Decision

Ship an addon (`addon/wowdps.lua` + a TOC template), embedded in the
daemon with `include_str!`, and treat its output as a **side table joined
at read time**:

- The addon records only inside a raid instance off LFR (keystone dungeons
  opt-in), keys every record by the unit GUID the log uses, writes the
  account's own characters too, and never writes a unit it cannot answer
  `GetGuildInfo` for — unknown is not unguilded.
- The daemon's history thread reads every account's `wowdps.lua` on start
  and on a 30 s idle poll, keeps the newest sighting per guid as
  `affiliations/<guid>.json`, and stamps `guild` onto a card's players when
  the card is **answered**. A card never stores a guild: the file lands
  after the night, so a stored value would be stale on every card written
  before it. `CardPlayer::guild` therefore travels on the wire (v31) but
  `to_json` skips it.
- Installing is the user's call once (`wowdps addon install`, into the
  product directory derived from the tailed logs dir and validated as an
  install, with `## Interface:` read from `.build.info`). The daemon only
  ever **updates**: on start it rewrites a copy that is not byte-for-byte
  what it would write, and leaves a missing one missing.
- The addon's own-character set answers "who is me" before the
  COMBATANT_INFO intersection does, which makes one log enough.
- SQL gets the same files as an `affiliations` view; the guild-night recipe
  joins it to `players`, and the parity gate runs it over a fake install.

## Consequences

Guilds lag by one logout, and `Status` says so (`affiliations_utc_ms`).
A player who leaves their guild is known only once the addon sees them
again in a raid. Reading SavedVariables needed a Lua-table reader in
`proto` — a reader of the game's serializer output, never an interpreter.
The daemon writes into the game install, but only to a folder the user
created by asking, and only with the bytes it embeds.

[^contract]: `CONTRACT.md`, wire history row v31 and the history-store paragraph.
[^spec]: `docs/spec-history-store.md` §9a.
