# History store — worked queries

The recipe list `docs/spec-role-pivots.md` §9 promised: one statement per
question over the views `wowdps history` defines (`wowdps history views`
lists the ones your lake has — a rows-tier view exists only once a file
carries the field, typed; `wowdps history regrade --kind all` back-fills
an older lake). The MCP's `history_sql` runs the same SQL, with `$1…`
bound parameters. Every query here is executed by
`crates/history/tests/parity.rs` over the fixture lake, so it cannot rot.

## Healer rank trend across a tier

```sql
select f.start_utc_ms, f.name, r.rank, r.count, r.measure
from role_ranks r join fights f on f.id = r.fight_id
where r.guid = $1 and r.role = 'healer' and not f.aborted
order by f.start_utc_ms;
```

## Externals given, to whom, how early (R18, step 4b)

```sql
select u.fight_id, u.label as spell, t.name as target, t.role,
       u.count, u.total_ms / 1000.0 as secs
from uptime u
join players t on t.fight_id = u.fight_id and t.guid = u.guid
where u.src = $1 and u.kind = 'external'
order by u.fight_id, secs desc;
```

`uptime` is keyed by the TARGET (`guid`); the caster is `src`. For the
timing of each cast, unnest the target's mark list:

```sql
select c.fight_id, m.at_ms / 1000.0 as at_secs, m.dur_ms / 1000.0 as secs,
       m.label, m.src as caster
from coarse c, unnest(c.marks) as u(m)
where c.guid = $1 and m.kind = 3          -- MarkKind::External's code
order by c.fight_id, at_secs;
```

## Active-mitigation uptime vs damage taken (tanks)

```sql
select p.fight_id, p.name, p.am_uptime_pct_sql as am_pct,
       m.mitigated_pct, p.dtps
from players p join mitigation m on m.fight_id = p.fight_id and m.guid = p.guid
where p.role = 'tank' and not p.enemy
order by p.fight_id;
```

`am_uptime_pct_sql` is recomputed from the stored `am_uptime_ms`, so a
card written before step 4b reads 0 (see `stats.cards_without_am_uptime`).

## Tank swap points (10 s taken series per tank)

```sql
select c.fight_id, p.name, i - 1 as bucket, c.taken10[i] as taken
from coarse c
join players p on p.fight_id = c.fight_id and p.guid = c.guid,
     unnest(generate_series(1, len(c.taken10))) as u(i)
where p.role = 'tank' and c.fight_id = $1
order by bucket, p.name;
```

## Support uptime per target (Augmentation)

```sql
select u.fight_id, t.name as target, t.spec_name, u.label,
       u.total_ms * 100.0 / f.duration_ms as uptime_pct
from uptime u
join fights f on f.id = u.fight_id
join players t on t.fight_id = u.fight_id and t.guid = u.guid
where u.src = $1 and u.kind = 'support_buff'
order by u.fight_id, uptime_pct desc;
```

## Augmentation contribution per target (R19)

```sql
select s.fight_id, s.target, t.name, t.spec_name, s.damage, s.healing
from support_targets s
join players t on t.fight_id = s.fight_id and t.guid = s.target
where s.guid = $1
order by s.fight_id, s.damage desc;
```

## Effective DPS for the buffed

```sql
select fight_id, name, dps, effective_dps_sql
from players
where role = 'dps' and not enemy and support_received > 0
order by fight_id, effective_dps_sql desc;
```

## Damage taken by ability, avoidable share

```sql
select s.fight_id, s.label, sum(s.amount) as taken
from taken_spells s
where s.guid = $1
group by 1, 2 order by taken desc;
```

Join a reader-supplied avoidable list (roadmap item 2): the store holds
the facts, not the verdict.
