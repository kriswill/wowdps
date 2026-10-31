---
type: Tool
title: gen-placed-spells
description: 'Regenerate proto''s table of what players place in the room (totems, gateways, rings, zones, statues, sigils, traps): the curated list proven by the client''s spell names, its reach and duration read from the client''s spell tables, and each entry backed by a real-log census, so the cut keeps their lines and the replay draws them.'
resource: tools/gen-placed-spells.sh
tags: [tool, game-data, replay]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-10-07T23:40:00-07:00 }
sources:
  - id: script
    resource: ../../../tools/gen-placed-spells.sh
    title: tools/gen-placed-spells.sh — the manual, in its header
  - id: code
    resource: ../../../tools/extract/src/placegen.rs
    title: placegen.rs — the curated list, its proof and the emitted table
  - id: types
    resource: ../../../crates/proto/src/placed.rs
    title: wowdps_proto::placed — Placed, Found, Form, and the lookups
  - id: twin
    resource: ../../../crates/proto/src/placed/placed_spells.expected.md
    title: The review twin — every entry's sources and census counts
---

`tools/gen-placed-spells.sh` builds the extractor and runs `wowdps-extract
gen-placed-spells`[^script] over the install's SpellName, SpellMisc,
SpellDuration, SpellEffect, SpellRadius, SpellCooldowns, SpellCategories and
SpellCategory, with the committed census
[census-placed-spells](census-placed-spells.md) writes. It writes
`crates/proto/src/placed/placed_spells.rs` and its review twin[^twin].
The types are [wowdps-proto](../crates/proto.md)'s `placed`
module[^types]: the daemon, whose replay cut keeps these spells' lines,
and the replay, a pure client that draws what they place, both read
proto, so the table needs neither engine nor GUI code. It was the
encounter rubric's until the rubric left wowdps with the replay
(2026-10-30).

## An entry

The membership is curated in `placegen.rs`[^code]. An entry names its
placing spell (the summon's own for a summoned unit, which SPELL_SUMMON
names) and says how the log tells where it stands, most trusted first:

- **`Unit`**: the unit its summon made, posted wherever it stands. Most
  totems post themselves at least 95 % of the time through their own
  casts and heals, so a totem moved by Totemic Projection is seen moved.
  Wind Rush, Surging Totem and Ring of Frost are summoned but never
  posted, so each lists what tells next.
- **`Touch(spells)`**: where the units those spells reached stood: an aura
  on them, or a heal on them (Surging Totem's own Healing Rain).
- **`Caster`**, **`Target`**: where the caster, or the unit the cast was
  aimed at, stood.
- **`Trips(aura)`**: a Demonic Gateway's far gate, from its travelers'
  jumps. Each traveler applies 113942 to themselves, so the aura names the
  traveler, never the warlock.
- **`Thrown`**: Ring of Peace, whose only log line is its cast. It is
  placed where the enemies it threw out were thrown from.

Its form (an area, a ring, two gates, a post) says how it is drawn.

## What the client gives, and what it does not

- **Reach** is the largest radius among a named spell's effects. For a
  totem that is its aura's, not its summon's: Earthbind's summon carries
  the 8 yd circle a player aims with, its slow reaches 10.
- **Duration** is a named spell's: the gates' own summons (113886 / 113890,
  15 minutes), where the gateway's cast lasts 1 ms.
- **Cooldown** is the placing spell's: its own, its category's or its
  charges' recharge (SpellCooldowns, SpellCategories, SpellCategory),
  whichever is longest, before talents. The replay's When list tells a
  placement that is a major cooldown, a minute or more, beside every
  raid's wall and crowd control.
- **By hand**, marked so in the twin: Darkness's 8 yd and Solar Beam's
  5 yd (each a server-side area trigger whose shape the client does not
  ship), and how long a trap (60 s) or an Angelic Feather (15 s) waits to
  be touched.

## Left out

Damage carpets every pull is covered in (Consecration, Death and Decay,
Sigil of Flame, Blizzard), and what no committed log holds (Earthen Wall
and Ancestral Protection totems). An entry joins once a census carries it.

[^script]: `tools/gen-placed-spells.sh`'s header: what it reads, and the census it takes.
[^code]: `tools/extract/src/placegen.rs`: `CURATED`, `Census`, `generate_curated`, `emit`.
[^types]: `crates/proto/src/placed.rs`: `Placed`, `Found`, `Form`, `placed`, `tells`.
[^twin]: `crates/proto/src/placed/placed_spells.expected.md`: per entry, where its reach and duration came from, and its census counts.
