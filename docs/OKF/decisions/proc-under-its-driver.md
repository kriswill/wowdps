---
type: Decision
title: A Talent Proc Nests Under Its Driver
description: 'Talent procs hang under the spell that drives them (Blackened Soul under Wither) through a CURATED table proven from both sides — the client''s own spell text or a trigger effect, and a real-log census of the pair — because the client data links no proc to its driver; the new group kind is PROTO_VERSION 37.'
tags: [core, wire, game-data]
status: stable
generated: { by: claude-code/opus-5.5, at: 2026-09-30T08:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — R26 (step 4) and the wire table's v37 row
  - id: expected
    resource: ../../../crates/core/src/proc_spells.expected.md
    title: proc_spells.expected.md — every entry with its text evidence and census counts
  - id: census
    resource: ../../../tools/proc-spells-census.csv
    title: The committed proc census over nine real logs
---

**Where:** [`wowdps-core`](../crates/core.md) (`Segment::spell_tree`, the
generated `proc_spells.rs`), [`wowdps-model`](../crates/model.md)
(`GroupKind::Spell`), [`wowdps-proto`](../crates/proto.md) (`PROTO_VERSION`
37), [`tools/extract`](../crates/extract.md) (`procgen.rs`),
[gen-proc-spells](../tools/gen-proc-spells.md) and
[census-proc-spells](../tools/census-proc-spells.md); ruling
[R26](../rulings/r26.md), gated by [tree.txt](../fixtures/tree.md).

## Context

The ability tree ([The Ability Tree In The Inspector](ability-tree-in-the-inspector.md))
put pets under their summons and trinket effects under their items, but Warcraft
Logs' pane — the model the user pointed at — also opens Wither into Blackened
Soul. Nothing the generators read says Blackened Soul belongs to Wither: the
proc hangs off the talent's aura, whose trigger conditions are proc flags and
script. The obvious data route fails on inspection — the talents' proc auras
carry class masks that select no spell — and the handoff's standing rule was
"do not guess relationships".

## Decision

Curate, and prove every entry from BOTH sides before it ships, the way
[gen-role-spells](../tools/gen-role-spells.md) does for R18:

- **The client's text.** `procgen` follows the proc's description through its
  `$@spelldesc` references ("Your Blade of Justice causes the target to
  burn…" is Expurgation's), and one hop into the spells it references (an
  Overload's text reads Mastery: Elemental Overload's values, and that
  mastery's — or Mountains Will Fall's — text names the spell); or the text IS
  the driver's (an Erupt's description is its plague's); or a SpellEffect of
  the driver triggers the proc (Wake of Ashes → Truth's Wake). None of those
  is a build failure: "a guess, not a curation".
- **The logs.** The census counts each pair per log with two metrics, because
  procs relate to their drivers in two ways: a direct onset within 1.5 s after
  the same player's driver (Expurgation 99.9%, the Overloads 100% — their
  travel time is why the window is not 250 ms), or a hit landing on a target
  carrying the player's driver aura (Blackened Soul on Wither 99.9%, the
  Erupts 98–100%: they fire as a stack collapses or a host dies, never on a
  clock). The better must reach 90%.[^census] Pets fold onto their owners
  through SPELL_SUMMON — in 12.x a damage line's advanced block describes the
  target, so its owner field cannot — which is what shows Blighted Maw riding
  the Dreadstalkers' Dreadbite at 100%.

The specs are the ones most often in the owner's own keys and raids (the
history store, 2026-09-30): Demonology, Elemental, Arms, Unholy, Shadow,
Retribution, Holy Paladin, Balance, plus Wither's Hellcaller procs. Sixteen
entries shipped.[^expected] Left out, each for a reason on record: procs with
two or more drivers (Atonement, Beacon transfers, Psychic Link, Shooting
Stars, Deep Wounds, Earthliving; Soulburst, whose Consume becomes Devour in
Void Metamorphosis — 45% on Consume alone); Reap the Storm, whose text says
Cleave but whose log says otherwise; Sun's Avatar, whose beams hit whoever
crosses them (86% on either metric); and Beast Mastery's Boar Charge — proven
(98.6% after Kill Command) but Kill Command's damage row is the pet's, and a
boar under the hunter's wolf would mislead.

The tree matches the driver by NAME, as every row is keyed: the player's own
row, else the first pet row wearing it. The proc joins that row's group (so
Blighted Maw lands in "Call Dreadstalkers"), or the two form a group of the
new kind `Spell`, labelled by the driver and wearing the driver row's icon; a
proc whose driver dealt nothing stays a plain row.[^contract]

## Consequences

The group kind is a new wire code, and v36 was already deployed, so a v36
client would refuse a v37 daemon's breakdowns with `BadTag(3)` — hence
`PROTO_VERSION` 37 and the renamed socket rather than an in-place extension.
No field changed; the details tier writes `"kind":"spell"`, and a tree stored
before v37 simply has no such group until `wowdps history regrade` rewrites
it.

Growing the table is the same loop every time: curate in `procgen.rs`, run
`tools/census-proc-spells.sh` over logs that hold the spec (it counts only
curated pairs, so it takes about a minute over nine logs), commit the CSV,
run `tools/gen-proc-spells.sh`. A patch that renames a spell or drops its
text link fails the generator by name; a pair no committed log exercises
needs a `census_exempt` reason, printed beside its counts.

[^contract]: CONTRACT.md — R26 (step 4) and the wire table's v37 row
[^expected]: proc_spells.expected.md — every entry with its text evidence and census counts
[^census]: The committed proc census over nine real logs
