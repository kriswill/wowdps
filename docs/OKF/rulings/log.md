# Log

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
