# Log

## 2026-09-28

- **Update** — [R25 Raid timeline](r25.md): review fixes — a stored fight splits
  its killing blow's label only on a known attacker, so a nil-source ability
  named with a parenthetical stays whole; a resurrect naming no spell is no
  rez, live or stored; a synthetic pull with a battle rez and a self-rez now
  gates stored = live; and only window and mcp sessions are sent the
  timeline.
- **Creation** — [R25 Raid timeline](r25.md): the whole group's fight on every
  meter snapshot — the view's raid series, the deaths in order with their
  killing blows and rezzes, the lust windows — read-time over R9, R12, R17,
  R18 and R23 state; with it R9's recap entries carry their time before
  the death. [R23 Death spans](r23.md) now remember the resurrect's spell
  and rezzer for it.

## 2026-09-14

- **Update** — [R24 Enemy damage taken](r24.md): review fixes — summons-only destination gate, attackers keyed by owner guid and folded at read, snapped windows, no compare, drill closes across the keyspace.
- **Update** — [R24 Enemy damage taken](r24.md): a zoom window on the drill's graph scopes its rows (PROTO_VERSION 33).
- **Update** — [R24 Enemy damage taken](r24.md): the drill is attackers first, then one attacker's abilities and curve.
- **Creation** — [R24 Enemy damage taken](r24.md): a third record of every hit,
  on the ENEMY it landed on, one row per enemy name — the game's own enemy
  meter. Shaped by two fixture edges: relog.txt's orphaned pet and
  taken.txt's Niuzao (ours, though a `Creature-`).
- **Creation** — [R23 Death spans](r23.md), scaffolded from its CONTRACT.md row.

## 2026-09-06

- **Creation** — [R22 Self-harm](r22.md): damage an actor deals to itself (own
  pets folded) leaves the Damage view for a `self_harm` tally. Found by
  comparing a real +14 against the group's in-game meters — a Brewmaster read
  89.4M to their 66.7M, 18.7M of it his own and Niuzao's Stagger ticks.
- **Creation** — [R21 Stacked-debuff conditioning](r21.md), scaffolded from the
  CONTRACT.md row it shipped with.
- **Update** — [R1 Damage](r1.md) and [R17 Damage taken & mitigation](r17.md)
  carry the R22 amendment and the restated identity.
