---
type: Decision
title: Cooldown Windows Belong To The View They Move
description: Every spec's major cooldown, long defensive, self-shield and healing cooldown is a curated R18 span (v34 adds the `HealingCooldown` kind), and a GUI graph draws only the marks its VIEW is about — cooldowns on the damage curve, walls on the taken curve, healing windows on the healing curve — instead of every mark on every curve.
tags: [meter, gui, wire, game-data]
status: stable
generated: { by: claude-fable/5.1, at: 2026-09-21T12:00:00-07:00 }
sources:
  - id: contract
    resource: ../../../CONTRACT.md
    title: CONTRACT.md — rulings table row R18 and the wire version table row 34
  - id: rolegen
    resource: ../../../tools/extract/src/rolegen.rs
    title: the curated membership behind role_spells.rs
---

**Where:** [R18 Aura spans with caster & target](../rulings/r18.md), [`wowdps-model`](../crates/model.md), [`wowdps-core`](../crates/core.md), [`wowdps-gui`](../crates/gui.md), [gen-role-spells](../tools/gen-role-spells.md), [census-role-spells](../tools/census-role-spells.md).

## Context

The question was "when did each player press their big cooldown, and for how
long was it up?" — Summon Demonic Tyrant on a Demonology Warlock being the
example — with the same question for long defensives (Icebound Fortitude,
Ice Barrier, health potions, walls) on the damage-taken graph and for the
healers' cooldowns on the healing graph. R18 already opened a span for every
aura in the curated role table and every timeline flavor already carried
every mark; what was missing was breadth (the table held seventy spells, one
or two cooldowns per class, no self-shields) and a graph that could tell a
cooldown's window from a wall's.

## Decision

- **One more kind, not a role gate.** `RoleSpellKind::HealingCooldown` /
  `MarkKind::HealingCooldown` (code 9, after `Death`, name
  `healing_cooldown`) names a healing cooldown's own buff. A hybrid window
  (Avenging Wrath, Convoke the Spirits) stays `Cooldown`: the kind says what
  the buff IS, and both throughput graphs draw a `Cooldown`, so the pally's
  wings read on their healing curve without a class or spec check the meter
  never had.
- **The table covers every spec.** The census behind it grew from three logs
  to eight (2 046 aura ids applied to players), and the curated list from 70
  to 138 entries: every spec's major offensive cooldown, its long defensive,
  the mage barriers and the raid-wide walls (Power Word: Barrier, Zephyr),
  and the healing cooldowns. A spec no committed log holds a player of ships
  `census_exempt` under one reason (`UNSEEN`) with name and APPLY_AURA still
  proven — the first such player's graph must not be blank. Cast ids without
  an aura (Empower Rune Weapon) and names the client no longer carries
  (Crusade is "Avenging Wrath" in 12.1) fell out of the list under the
  generator's own checks, as designed.
- **The renderer filters, the daemon does not.** `Segment::timeline` /
  `heal_timeline` / `taken_timeline` still carry every mark (R18 unchanged,
  the wire unchanged in shape); `compare::view_draws(view, kind)` decides
  what a graph draws: items, externals and deaths on every curve; `Cooldown`
  and `SupportBuff` on Damage and Healing, never on Taken; `HealingCooldown`
  on Healing alone; `ActiveMitigation` and `Defensive` on Taken alone. The
  legend follows the drawn marks, so a Taken graph never keys a cooldown.
  Health potions and healthstones were already R12 consumable marks and now
  read where they belong, beside the defensives on the taken curve.

## Consequences

- `PROTO_VERSION` 34 (the socket renames); a v33 client rejects a code-9
  mark, which is what the rename prevents. Stored records write
  `"kind":"healing_cooldown"`; a pre-v34 reader drops it as unknown.
- The mcp tools and the DuckDB `uptime` view answer `healing_cooldown` by
  name; nothing else in the store changed shape.
- Pre-v34 fight cards were graded from the old table, so a healer's
  `uptime[]` on a stored fight shows healing cooldowns only after
  `regrade_fights`.
