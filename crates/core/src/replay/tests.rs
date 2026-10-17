use super::*;

/// An event as the tests compare it: time, kind, unit, spell, src, target.
type EventRow = (
    u32,
    EventKind,
    Option<UnitId>,
    u32,
    Option<UnitId>,
    Option<UnitId>,
);
/// A placed row: time, kind, unit, spell, where, src, target.
type PlacedRow = (
    u32,
    PlacedKind,
    Option<UnitId>,
    u32,
    Option<(i32, i32)>,
    Option<UnitId>,
    Option<UnitId>,
);

/// A placed table of the test's own: Demonic Gateway (cast) and its trip
/// aura, Wind Rush Totem (summon) and its buff, Anti-Magic Zone's buff and
/// Healing Rain's heal (both telling), Demonic Circle (a create).
struct Table;

impl PlacedTable for Table {
    fn places(&self, spell: u32) -> bool {
        matches!(spell, 111_771 | 192_077 | 48_018)
    }
    fn tells(&self, spell: u32) -> bool {
        matches!(spell, 113_942 | 192_082 | 145_629 | 73_921)
    }
}

/// A pull of encounter 3000 (a kill, 12 s) after a wipe of it, its lines
/// as the game ends them (CR LF): zones, world markers, a player and a pet,
/// a boss and its add, a totem (friendly), and every line the cut keeps.
const LOG: &str = concat!(
    "10/7/2026 20:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1\r\n",
    "10/7/2026 20:00:01.000-7  ZONE_CHANGE,3004,\"The Venomous Abyss\",16\r\n",
    "10/7/2026 20:00:02.000-7  WORLD_MARKER_PLACED,3004,4,100.50,-20.25\r\n",
    "10/7/2026 20:00:02.500-7  WORLD_MARKER_PLACED,3004,1,90.00,-10.00\r\n",
    "10/7/2026 20:00:03.000-7  WORLD_MARKER_PLACED,9999,2,1.00,2.00\r\n",
    "10/7/2026 20:00:05.000-7  ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004\r\n",
    // A COMBATANT_INFO for the healer in the WIPE, none in the kill: the
    // seeds keep it.
    "10/7/2026 20:00:05.100-7  COMBATANT_INFO,Player-1-B,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,105,0\r\n",
    "10/7/2026 20:00:06.000-7  ENCOUNTER_END,3000,\"Ula'tek\",16,20,0,1000\r\n",
    "10/7/2026 20:01:00.000-7  ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004\r\n",
    "10/7/2026 20:01:00.100-7  COMBATANT_INFO,Player-1-A,0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,250,0\r\n",
    // The player cast at the boss: the advanced block describes the caster.
    "10/7/2026 20:01:00.500-7  SPELL_CAST_SUCCESS,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,49998,\"Death Strike\",0x1,Player-1-A,0000000000000000,900,1000,1,2,3,4,5,6,0,100,100,0,1500.25,-3.50,2434,1.5000,90\r\n",
    // The boss hits the player: the block describes the victim.
    "10/7/2026 20:01:01.000-7  SPELL_DAMAGE,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7000,\"Venom\",0x8,Player-1-A,0000000000000000,800,1000,1,2,3,4,5,6,0,100,100,0,1501.00,-3.00,2434,1.6000,90,100,120,-1,8,0,0,0,nil,nil,nil\r\n",
    "10/7/2026 20:01:02.000-7  SPELL_CAST_START,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,0000000000000000,nil,0x80000000,0x80000000,7001,\"Spit\",0x8\r\n",
    "10/7/2026 20:01:02.500-7  SPELL_CAST_SUCCESS,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7001,\"Spit\",0x8,Creature-0-1-2-3-500-AA,0000000000000000,9000,10000,0,0,0,0,0,0,3|4,25|1,100|5,0,1510.00,0.00,2434,3.1416,90\r\n",
    // An add, a tenth of the boss's health, with no pool (`1,0,0`).
    "10/7/2026 20:01:03.000-7  SPELL_CAST_SUCCESS,Creature-0-1-2-3-501-BB,\"Egg\",0xa48,0x80000000,0000000000000000,nil,0x80000000,0x80000000,7002,\"Hatch\",0x8,Creature-0-1-2-3-501-BB,0000000000000000,1000,1000,0,0,0,0,0,0,1,0,0,0,1520.00,5.00,2434,0.0000,90\r\n",
    // A totem's own cast (friendly).
    "10/7/2026 20:01:04.000-7  SPELL_CAST_SUCCESS,Creature-0-1-2-3-5394-CC,\"Healing Stream Totem\",0x2111,0x80000000,0000000000000000,nil,0x80000000,0x80000000,5394,\"Healing Stream\",0x8,Creature-0-1-2-3-5394-CC,Player-1-A,10,10,0,0,0,0,0,0,0,0,0,0,1499.00,-2.00,2434,0.5000,90\r\n",
    // The same player on another floor once: posted there, and the hit
    // that found them there a row.
    "10/7/2026 20:01:04.500-7  SPELL_DAMAGE,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7000,\"Venom\",0x8,Player-1-A,0000000000000000,700,1000,1,2,3,4,5,6,0,100,100,0,10.00,20.00,2435,0.0000,90,100,120,-1,8,0,0,0,nil,nil,nil\r\n",
    // A debuff from the boss, its stacks and its removal.
    "10/7/2026 20:01:05.000-7  SPELL_AURA_APPLIED,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7003,\"Rot\",0x8,DEBUFF\r\n",
    "10/7/2026 20:01:05.500-7  SPELL_AURA_APPLIED_DOSE,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7003,\"Rot\",0x8,DEBUFF,2\r\n",
    "10/7/2026 20:01:06.000-7  WORLD_MARKER_REMOVED,4\r\n",
    "10/7/2026 20:01:06.250-7  WORLD_MARKER_PLACED,3004,7,110.00,-30.00\r\n",
    "10/7/2026 20:01:06.500-7  SPELL_CAST_FAILED,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,0000000000000000,nil,0x80000000,0x80000000,49998,\"Death Strike\",0x1,\"Not enough runic power\"\r\n",
    "10/7/2026 20:01:07.000-7  SPELL_AURA_REMOVED,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7003,\"Rot\",0x8,DEBUFF\r\n",
    // A shield took a hit whole: a miss, no position.
    "10/7/2026 20:01:08.000-7  SPELL_MISSED,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,7000,\"Venom\",0x8,ABSORB,nil,50,60,nil\r\n",
    "10/7/2026 20:01:09.000-7  SPELL_INTERRUPT,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000,47528,\"Mind Freeze\",0x10,7001,\"Spit\",0x8\r\n",
    "10/7/2026 20:01:10.000-7  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Creature-0-1-2-3-501-BB,\"Egg\",0xa48,0x80000000,0\r\n",
    // A hunter feigning: no death.
    "10/7/2026 20:01:10.500-7  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1-H,\"Hunt-Realm-US\",0x512,0x80000000,1\r\n",
    "10/7/2026 20:01:11.000-7  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,0\r\n",
    "10/7/2026 20:01:11.500-7  SPELL_RESURRECT,Player-1-B,\"Healer-Realm-US\",0x512,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,20484,\"Rebirth\",0x8\r\n",
    "10/7/2026 20:01:12.000-7  ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,12000\r\n",
    "10/7/2026 20:02:00.000-7  ENCOUNTER_START,3001,\"Other\",16,20,3004\r\n",
);

/// The lines of `LOG` (its CR LF dropped, as the index's reader drops
/// them) from `from` up to (not including) `to`.
fn lines(from: usize, to: usize) -> Vec<&'static str> {
    LOG.split("\r\n").skip(from).take(to - from).collect()
}

/// The second pull, cut with every earlier line as its seeds.
fn second() -> Cut {
    cut(lines(0, 8), lines(8, 30), &Table, Some("Player-1-A"))
}

#[test]
fn the_head_is_the_pulls_own() {
    let c = second();
    assert_eq!(
        c.head.encounter,
        Some(EncounterHead {
            id: 3000,
            name: "Ula'tek".into(),
            difficulty: 16,
            size: 20
        })
    );
    assert_eq!(
        (
            c.head.map,
            c.head.zone.as_str(),
            c.head.success,
            c.head.fight_ms
        ),
        (3004, "The Venomous Abyss", Some(true), Some(12_000))
    );
    // 20:01 at UTC-7 is 03:01 UTC the next day.
    assert_eq!(c.head.date, (2026, 10, 7));
    assert_eq!(c.head.start_utc_ms % 86_400_000, (3 * 3600 + 60) * 1000);
    assert_eq!(c.floor, 2434);
}

#[test]
fn units_are_numbered_where_first_seen() {
    let c = second();
    let units: Vec<(UnitKind, &str, u32)> = c
        .units
        .iter()
        .map(|u| (u.kind, u.name.as_str(), u.npc))
        .collect();
    assert_eq!(
        units,
        [
            (UnitKind::Player, "Tank-Realm-US", 0),
            (UnitKind::Boss, "Ula'tek", 500),
            (UnitKind::Add, "Egg", 501),
            (UnitKind::Friendly, "Healing Stream Totem", 5394),
            // Named by the log, its spec from the wipe's COMBATANT_INFO.
            (UnitKind::Player, "Healer-Realm-US", 0),
        ]
    );
    let a = &c.units[0];
    assert_eq!(
        (a.class, a.spec, a.you),
        (Some(Class::DeathKnight), Some(Spec::Blood), true)
    );
    let b = &c.units[4];
    assert_eq!(
        (b.class, b.spec, b.you),
        (Some(Class::Druid), Some(Spec::RestorationDruid), false)
    );
    assert_eq!(
        (b.guid.as_str(), c.units[1].guid.as_str()),
        ("Player-1-B", "")
    );
}

#[test]
fn posts_keep_every_floor_with_health_and_power() {
    let c = second();
    let p = |t_ms, x, y, facing, hp, power: Option<(u32, u64, u64)>, map_id| {
        let power = power.map(|(kind, current, max)| Power { kind, current, max });
        Post {
            t_ms,
            x,
            y,
            facing,
            hp,
            power,
            map_id,
            absorb: 0,
            pools: power.into_iter().collect(),
        }
    };
    // The tank's blocks carry 6 in field 9: his shields.
    let shielded = |post: Post| Post { absorb: 6, ..post };
    let tank = [
        p(500, 150_025, -350, 15_000, 900, Some((0, 100, 100)), 2434),
        p(1000, 150_100, -300, 16_000, 800, Some((0, 100, 100)), 2434),
        p(4500, 1000, 2000, 0, 700, Some((0, 100, 100)), 2435),
    ]
    .map(shielded);
    assert_eq!(
        c.units[0].posts, tank,
        "the post on floor 2435 is kept, with its floor"
    );
    // The boss's Spit names two pools (`3|4`): its post carries both.
    let spit = p(2500, 151_000, 0, 31_416, 900, Some((3, 25, 100)), 2434);
    let combo = Power {
        kind: 4,
        current: 1,
        max: 5,
    };
    assert_eq!(
        c.units[1].posts,
        [Post {
            pools: spit.pools.iter().copied().chain([combo]).collect(),
            ..spit
        }]
    );
    // No pool, no power (the text cutter wrote `1,0,0`).
    assert_eq!(
        c.units[2].posts,
        [p(3000, 152_000, 500, 0, 1000, None, 2434)]
    );
    assert_eq!(c.posts(), 6);
    let maps: Vec<(u32, usize, usize)> = c
        .maps()
        .iter()
        .map(|m| (m.map_id, m.posts, m.players))
        .collect();
    assert_eq!(maps, [(2434, 5, 2), (2435, 1, 1)]);
    assert_eq!(c.floor, 2434, "the players' most posted");
}

#[test]
fn events_are_the_rows_a_replay_draws() {
    let c = second();
    let rows: Vec<EventRow> = c
        .events
        .iter()
        .map(|e| (e.t_ms, e.kind, e.unit, e.spell_id, e.src, e.target))
        .collect();
    use EventKind::*;
    assert_eq!(
        rows,
        [
            (0, BossEngaged, None, 3000, None, None),
            (500, PcastSuccess, Some(0), 49_998, None, Some(1)),
            (1000, Hit, Some(0), 7000, Some(1), None),
            (2000, CastStart, Some(1), 7001, None, None),
            (2500, CastSuccess, Some(1), 7001, None, Some(0)),
            (3000, CastSuccess, Some(2), 7002, None, None),
            (4500, Hit, Some(0), 7000, Some(1), None),
            (5000, DebuffApplied, Some(0), 7003, Some(1), None),
            (5500, DebuffDose, Some(0), 7003, Some(1), None),
            (6500, PcastFailed, Some(0), 49_998, None, None),
            (7000, DebuffRemoved, Some(0), 7003, Some(1), None),
            (8000, Hit, Some(0), 7000, Some(1), None),
            (9000, Interrupt, Some(1), 7001, Some(0), None),
            (10_000, NpcDied, Some(2), 0, None, None),
            (11_000, Death, Some(0), 0, None, None),
            (11_500, Rez, Some(0), 20_484, Some(4), None),
            (12_000, BossKilled, None, 3000, None, None),
        ]
    );
    assert_eq!(
        c.events[0].spell, "Ula'tek",
        "a boss row names its encounter"
    );
    let hit = &c.events[2];
    assert_eq!(
        (hit.at, hit.base, hit.spell.as_str()),
        (Some((150_100, -300)), Some(120), "Venom")
    );
    // The hit that found the tank on floor 2435: where his post there says.
    assert_eq!(c.events[6].at, Some((1000, 2000)));
    let miss = &c.events[11];
    assert_eq!((miss.at, miss.base), (None, None));
    assert_eq!(c.events[8].stacks, Some(2));
}

#[test]
fn markers_standing_then_changing() {
    let c = second();
    let m = |t_ms, kind, marker, at| Marker {
        t_ms,
        kind,
        marker,
        at,
    };
    assert_eq!(
        c.markers,
        [
            m(0, MarkerKind::Placed, 1, Some((9000, -1000))),
            m(0, MarkerKind::Placed, 4, Some((10_050, -2025))),
            m(6000, MarkerKind::Removed, 4, None),
            m(6250, MarkerKind::Placed, 7, Some((11_000, -3000))),
        ]
    );
}

/// A marker's number is one object: moved to another map, it no longer
/// stands on the one it left, however the pull comes back there.
#[test]
fn a_marker_moved_to_another_map_leaves_the_first() {
    let mut seeds = lines(0, 8);
    seeds.push("10/7/2026 20:00:30.000-7  WORLD_MARKER_PLACED,9999,4,5.00,6.00");
    let c = cut(seeds, lines(8, 30), &Table, Some("Player-1-A"));
    let standing: Vec<u8> = c
        .markers
        .iter()
        .filter(|m| m.t_ms == 0)
        .map(|m| m.marker)
        .collect();
    assert_eq!(standing, [1]);
}

/// The index's seeds (every COMBATANT_INFO, ZONE_CHANGE and marker line)
/// make the same cut as every line before the slice does.
#[test]
fn the_seeds_alone_cut_as_the_whole_log_does() {
    let all = second();
    let seeds: Vec<&str> = lines(0, 8)
        .into_iter()
        .filter(|l| {
            [
                "COMBATANT_INFO",
                "ZONE_CHANGE",
                "WORLD_MARKER",
                "COMBAT_LOG_VERSION",
            ]
            .iter()
            .any(|e| l.contains(e))
        })
        .collect();
    assert_eq!(cut(seeds, lines(8, 30), &Table, Some("Player-1-A")), all);
}

#[test]
fn without_a_named_boss_the_strongest_hostiles_are_bosses() {
    let renamed: Vec<String> = lines(0, 30)
        .into_iter()
        .map(|l| l.replace("\"Ula'tek\",16,20", "\"Someone Else\",16,20"))
        .collect();
    let all: Vec<&str> = renamed.iter().map(String::as_str).collect();
    let c = cut(all[..8].to_vec(), all[8..].to_vec(), &NoPlaced, None);
    // The egg (1000 of the boss's 10000) is under 30%; nobody is "you".
    assert_eq!(c.units[1].kind, UnitKind::Boss);
    assert_eq!(c.units[2].kind, UnitKind::Add);
    assert!(c.units.iter().all(|u| !u.you));
}

/// A pull of what players place: a warlock's gateway and a trip through
/// it, a shaman's Wind Rush Totem (summoned, never posted) and its buff, an
/// Anti-Magic Zone's buff (no source), a Healing Rain heal (where its
/// target stood), a buff on a unit nothing posts, a Demonic Circle made,
/// and the totem destroyed.
const PLACED_LOG: &str = concat!(
    "10/7/2026 21:00:00.000-7  ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004\r\n",
    "10/7/2026 21:00:01.000-7  SPELL_CAST_SUCCESS,Player-1-W,\"Lock-R-US\",0x511,0x80000000,0000000000000000,nil,0x80000000,0x80000000,111771,\"Demonic Gateway\",0x20,Player-1-W,0000000000000000,10,10,0,0,0,0,0,0,0,0,0,0,100.00,200.00,2434,0.0000,90\r\n",
    "10/7/2026 21:00:02.000-7  SPELL_CAST_SUCCESS,Player-1-T,\"Trav-R-US\",0x511,0x80000000,0000000000000000,nil,0x80000000,0x80000000,133,\"Fireball\",0x4,Player-1-T,0000000000000000,10,10,0,0,0,0,0,0,0,0,0,0,101.00,201.00,2434,0.0000,90\r\n",
    "10/7/2026 21:00:02.500-7  SPELL_AURA_APPLIED,Player-1-T,\"Trav-R-US\",0x511,0x80000000,Player-1-T,\"Trav-R-US\",0x511,0x80000000,113942,\"Demonic Gateway\",0x1,DEBUFF\r\n",
    "10/7/2026 21:00:03.000-7  SPELL_CAST_SUCCESS,Player-1-S,\"Sham-R-US\",0x511,0x80000000,0000000000000000,nil,0x80000000,0x80000000,192077,\"Wind Rush Totem\",0x8,Player-1-S,0000000000000000,10,10,0,0,0,0,0,0,0,0,0,0,90.00,190.00,2434,0.0000,90\r\n",
    "10/7/2026 21:00:03.000-7  SPELL_SUMMON,Player-1-S,\"Sham-R-US\",0x511,0x80000000,Creature-0-1-2-3-97285-WR,\"Wind Rush Totem\",0x2111,0x80000000,192077,\"Wind Rush Totem\",0x8\r\n",
    "10/7/2026 21:00:04.000-7  SPELL_AURA_APPLIED,Creature-0-1-2-3-97285-WR,\"Wind Rush Totem\",0x2111,0x80000000,Player-1-T,\"Trav-R-US\",0x511,0x80000000,192082,\"Wind Rush\",0x8,BUFF\r\n",
    "10/7/2026 21:00:05.000-7  SPELL_AURA_APPLIED,0000000000000000,nil,0x80000000,0x80000000,Player-1-W,\"Lock-R-US\",0x511,0x80000000,145629,\"Anti-Magic Zone\",0x20,BUFF\r\n",
    "10/7/2026 21:00:05.500-7  SPELL_AURA_APPLIED,0000000000000000,nil,0x80000000,0x80000000,Pet-0-1-2-3-4-P,\"Pet\",0x1111,0x80000000,145629,\"Anti-Magic Zone\",0x20,BUFF\r\n",
    "10/7/2026 21:00:06.000-7  SPELL_HEAL,Player-1-S,\"Sham-R-US\",0x511,0x80000000,Player-1-W,\"Lock-R-US\",0x511,0x80000000,73921,\"Healing Rain\",0x8,Player-1-W,0000000000000000,10,10,0,0,0,0,0,0,0,0,0,0,95.50,195.25,2434,0.0000,90,100,100,0,0,nil\r\n",
    "10/7/2026 21:00:06.500-7  SPELL_CREATE,Player-1-W,\"Lock-R-US\",0x511,0x80000000,GameObject-0-1-2-3-191083-DC,\"Demonic Circle\",0x4228,0x80000000,48018,\"Demonic Circle\",0x20\r\n",
    "10/7/2026 21:00:07.000-7  UNIT_DESTROYED,0000000000000000,nil,0x80000000,0x80000000,Creature-0-1-2-3-97285-WR,\"Wind Rush Totem\",0x2111,0x80000000,0\r\n",
    "10/7/2026 21:00:08.000-7  ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,8000\r\n",
);

#[test]
fn placed_is_what_players_placed_and_what_tells_where() {
    let c = cut(std::iter::empty(), PLACED_LOG.split("\r\n"), &Table, None);
    let rows: Vec<PlacedRow> = c
        .placed
        .iter()
        .map(|p| (p.t_ms, p.kind, p.unit, p.spell_id, p.at, p.src, p.target))
        .collect();
    use PlacedKind::*;
    // Units: the warlock, the traveler, the shaman (posted), then the totem
    // and the circle the summons are first to name.
    assert_eq!(
        rows,
        [
            (
                1000,
                Cast,
                Some(0),
                111_771,
                Some((10_000, 20_000)),
                None,
                None
            ),
            (2500, Touch, Some(1), 113_942, None, Some(1), None),
            (
                3000,
                Cast,
                Some(2),
                192_077,
                Some((9000, 19_000)),
                None,
                None
            ),
            (3000, Summon, Some(2), 192_077, None, None, Some(3)),
            (4000, Touch, Some(1), 192_082, None, Some(3), None),
            (5000, Touch, Some(0), 145_629, None, None, None),
            (
                6000,
                Touch,
                Some(0),
                73_921,
                Some((9550, 19_525)),
                Some(2),
                None
            ),
            (6500, Summon, Some(0), 48_018, None, None, Some(4)),
            (7000, Gone, Some(3), 0, None, None, None),
        ]
    );
    assert_eq!(c.units.len(), 5);
    assert_eq!(
        (c.units[3].kind, c.units[3].name.as_str(), c.units[3].npc),
        (UnitKind::Friendly, "Wind Rush Totem", 97_285)
    );
    assert!(c.units[3].posts.is_empty());
    // Nothing else reads differently for it.
    assert!(c.events.iter().all(|e| e.spell_id != 192_082));
}

#[test]
fn a_title_is_split_at_commas_and_a_lone_and() {
    let p = title_parts("Kyrakka and Erkhart Stormvein");
    for want in [
        "kyrakka and erkhart stormvein",
        "kyrakka",
        "erkhart stormvein",
    ] {
        assert!(p.iter().any(|x| x == want), "{want} in {p:?}");
    }
    let p = title_parts("Vanguard, Brand and Sand");
    for want in ["vanguard", "brand", "sand"] {
        assert!(p.iter().any(|x| x == want), "{want} in {p:?}");
    }
    assert_eq!(
        title_parts("Ula'tek").len(),
        2,
        "the whole and its one piece"
    );
}

#[test]
fn an_npc_id_is_a_guids_sixth_field() {
    assert_eq!(
        npc_id("Creature-0-3883-2813-74658-235597-0000110932"),
        Some(235_597)
    );
    assert_eq!(
        npc_id("Pet-0-3883-2813-74658-165189-0105A3B2C1"),
        Some(165_189)
    );
    assert_eq!(npc_id("Player-5-0EB6ECFC"), None);
    assert_eq!(npc_id("0000000000000000"), None);
}

#[test]
fn health_reads_as_the_files_one_decimal() {
    assert_eq!(hp_tenths(900, 1000), 900);
    assert_eq!(hp_tenths(0, 1000), 0);
    assert_eq!(hp_tenths(1, 3), 333);
    assert_eq!(hp_tenths(2, 3), 667);
    // 12.35 % is 12.3499… as a float: the file says 12.3, so this does.
    assert_eq!(hp_tenths(1235, 10_000), 123);
    assert_eq!(hp_tenths(5, 0), 0);
}

#[test]
fn a_local_date_is_its_civil_day() {
    let ms = |days: i64| days * 86_400_000 + 1;
    assert_eq!(civil_date(ms(0)), (1970, 1, 1));
    assert_eq!(civil_date(ms(20_370)), (2025, 10, 9));
    assert_eq!(civil_date(ms(-1)), (1969, 12, 31));
}

/// A creature going down unconscious is down; a hunter feigning is not.
#[test]
fn unconscious_npcs_go_down_feigning_players_do_not() {
    let log = concat!(
        "10/7/2026 22:00:00.000-7  ENCOUNTER_START,3001,\"Council\",16,20,3004\r\n",
        "10/7/2026 22:00:01.000-7  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Vehicle-0-1-2-3-257911-ZZ,\"Zul'jan\",0x10a48,0x80000000,1\r\n",
        "10/7/2026 22:00:02.000-7  UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-1-H,\"Hunt-Realm-US\",0x512,0x80000000,1\r\n",
        "10/7/2026 22:00:03.000-7  ENCOUNTER_END,3001,\"Council\",16,20,0,3000\r\n",
    );
    let c = cut(std::iter::empty(), log.split("\r\n"), &NoPlaced, None);
    let kinds: Vec<EventKind> = c.events.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [
            EventKind::BossEngaged,
            EventKind::NpcDied,
            EventKind::BossWiped
        ]
    );
    assert_eq!(c.head.success, Some(false));
}

/// With no hostile posted there is no health to weigh: no hostile is a
/// boss by health (an unnamed one is an add, never every one a boss).
#[test]
fn with_no_hostile_posted_none_is_a_boss_by_health() {
    let log = concat!(
        "10/7/2026 22:00:00.000-7  ENCOUNTER_START,3001,\"Council\",16,20,3004\r\n",
        "10/7/2026 22:00:01.000-7  SPELL_CAST_START,Creature-0-1-2-3-700-EE,\"Shade\",0xa48,0x80000000,0000000000000000,nil,0x80000000,0x80000000,7001,\"Spit\",0x8\r\n",
        "10/7/2026 22:00:02.000-7  SPELL_CAST_START,Creature-0-1-2-3-701-FF,\"Wisp\",0xa48,0x80000000,0000000000000000,nil,0x80000000,0x80000000,7002,\"Glow\",0x8\r\n",
        "10/7/2026 22:00:03.000-7  ENCOUNTER_END,3001,\"Council\",16,20,0,3000\r\n",
    );
    let c = cut(std::iter::empty(), log.split("\r\n"), &NoPlaced, None);
    let kinds: Vec<UnitKind> = c.units.iter().map(|u| u.kind).collect();
    assert_eq!(kinds, [UnitKind::Add, UnitKind::Add]);
}

/// The slice's first line opens the cut and is read as every other: a run
/// that opens on a hit keeps that hit's row and its victim's post at t 0.
#[test]
fn a_slice_opening_on_a_hit_keeps_its_row_and_post() {
    let c = cut(lines(0, 11), lines(11, 30), &Table, Some("Player-1-A"));
    let first = &c.events[0];
    assert_eq!(
        (first.t_ms, first.kind, first.spell.as_str()),
        (0, EventKind::Hit, "Venom")
    );
    let tank = &c.units[first.unit.unwrap() as usize];
    assert_eq!(tank.name, "Tank-Realm-US");
    assert_eq!(tank.posts.first().map(|p| p.t_ms), Some(0));
}

/// A keystone run's slice, from its CHALLENGE_MODE_START: trash on the
/// dungeon's first floor, a boss on its second (the party steps down, and a
/// heal finds a player there), the run's end — every floor's posts kept
/// with their floors, the boss's start and end as rows mid-run, and the
/// main floor the players' most posted.
#[test]
fn a_keys_run_keeps_every_floor_and_its_bosses() {
    let log = concat!(
        "10/8/2026 19:00:00.000-7  ZONE_CHANGE,2521,\"Ruby Dungeon\",8\r\n",
        "10/8/2026 19:00:01.000-7  CHALLENGE_MODE_START,\"Ruby Dungeon\",2521,399,14,[10,9]\r\n",
        "10/8/2026 19:00:05.000-7  SPELL_DAMAGE,Creature-0-1-2-3-800-AA,\"Drake\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,8000,\"Flame\",0x4,Player-1-A,0000000000000000,900,1000,0,0,0,0,0,0,0,100,100,0,10.00,20.00,2094,1.0000,90,100,120,-1,4,0,0,0,nil,nil,nil\r\n",
        "10/8/2026 19:00:06.000-7  SPELL_CAST_SUCCESS,Player-1-B,\"Healer-Realm-US\",0x512,0x80000000,0000000000000000,nil,0x80000000,0x80000000,774,\"Rejuvenation\",0x8,Player-1-B,0000000000000000,1000,1000,0,0,0,0,0,0,0,100,100,0,11.00,21.00,2094,1.0000,90\r\n",
        "10/8/2026 19:00:07.000-7  SPELL_HEAL,Player-1-B,\"Healer-Realm-US\",0x512,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,73921,\"Healing Rain\",0x8,Player-1-A,0000000000000000,950,1000,0,0,0,0,0,0,0,100,100,0,12.00,22.00,2094,1.0000,90,50,50,0,0,nil\r\n",
        "10/8/2026 19:01:00.000-7  ENCOUNTER_START,2609,\"Frost Warden\",8,5,2521\r\n",
        "10/8/2026 19:01:01.000-7  SPELL_DAMAGE,Creature-0-1-2-3-900-BB,\"Frost Warden\",0xa48,0x80000000,Player-1-A,\"Tank-Realm-US\",0x511,0x80000000,9000,\"Frost\",0x10,Player-1-A,0000000000000000,800,1000,0,0,0,0,0,0,0,100,100,0,50.00,60.00,2095,2.0000,90,100,120,-1,16,0,0,0,nil,nil,nil\r\n",
        "10/8/2026 19:01:02.000-7  SPELL_CAST_SUCCESS,Player-1-B,\"Healer-Realm-US\",0x512,0x80000000,0000000000000000,nil,0x80000000,0x80000000,774,\"Rejuvenation\",0x8,Player-1-B,0000000000000000,1000,1000,0,0,0,0,0,0,0,100,100,0,52.00,61.00,2095,1.0000,90\r\n",
        "10/8/2026 19:01:03.000-7  SPELL_CAST_SUCCESS,Player-1-B,\"Healer-Realm-US\",0x512,0x80000000,0000000000000000,nil,0x80000000,0x80000000,774,\"Rejuvenation\",0x8,Player-1-B,0000000000000000,1000,1000,0,0,0,0,0,0,0,100,100,0,52.00,62.00,2095,1.0000,90\r\n",
        "10/8/2026 19:01:04.000-7  SPELL_CAST_SUCCESS,Player-1-B,\"Healer-Realm-US\",0x512,0x80000000,0000000000000000,nil,0x80000000,0x80000000,774,\"Rejuvenation\",0x8,Player-1-B,0000000000000000,1000,1000,0,0,0,0,0,0,0,100,100,0,52.00,63.00,2095,1.0000,90\r\n",
        "10/8/2026 19:02:00.000-7  ENCOUNTER_END,2609,\"Frost Warden\",8,5,1,60000\r\n",
        "10/8/2026 19:03:00.000-7  CHALLENGE_MODE_END,2521,1,14,179000,250.0,1000.0\r\n",
    );
    let lines: Vec<&str> = log.split("\r\n").collect();
    let c = cut(lines[..1].to_vec(), lines[1..].to_vec(), &Table, None);
    assert_eq!(c.head.encounter, None);
    assert_eq!(
        c.head.key,
        Some(KeyHead {
            name: "Ruby Dungeon".into(),
            level: 14,
            affixes: vec![10, 9],
        })
    );
    let bosses: Vec<(u32, EventKind, Option<UnitId>, u32, &str)> = c
        .events
        .iter()
        .filter(|e| e.kind.is_boss())
        .map(|e| (e.t_ms, e.kind, e.unit, e.spell_id, e.spell.as_str()))
        .collect();
    assert_eq!(
        bosses,
        [
            (59_000, EventKind::BossEngaged, None, 2609, "Frost Warden"),
            (119_000, EventKind::BossKilled, None, 2609, "Frost Warden"),
        ]
    );
    let maps: Vec<(u32, usize, usize)> = c
        .maps()
        .iter()
        .map(|m| (m.map_id, m.posts, m.players))
        .collect();
    assert_eq!(maps, [(2094, 3, 3), (2095, 4, 4)]);
    assert_eq!(c.floor, 2095, "the floor the players were posted on most");
    let tank = c.units.iter().find(|u| u.name == "Tank-Realm-US").unwrap();
    let floors: Vec<u32> = tank.posts.iter().map(|p| p.map_id).collect();
    assert_eq!(floors, [2094, 2094, 2095], "the tank steps down a floor");
    // Both hits are rows, each where its victim stood on its own floor —
    // the first off the main one — and the heal on the upper floor tells
    // where Healing Rain fell there.
    let hits: Vec<Option<(i32, i32)>> = c
        .events
        .iter()
        .filter(|e| e.kind == EventKind::Hit)
        .map(|e| e.at)
        .collect();
    assert_eq!(hits, [Some((1000, 2000)), Some((5000, 6000))]);
    let rain: Vec<Option<(i32, i32)>> = c.placed.iter().map(|p| p.at).collect();
    assert_eq!(rain, [Some((1200, 2200))]);
}

// ---- format 3: the buffs that matter, players' debuffs on NPCs, pools -------------

/// A line `body` at `t` (seconds past 21:00:00) on 10/7/2026.
fn at(t: &str, body: &str) -> String {
    format!("10/7/2026 21:00:{t}-7  {body}")
}

const TANK: &str = "Player-1-A,\"Tank-Realm-US\",0x511,0x80000000";
const HEALER: &str = "Player-1-B,\"Healer-Realm-US\",0x512,0x80000000";
const EVOKER: &str = "Player-1-E,\"Evoker-Realm-US\",0x512,0x80000000";
const BOSS: &str = "Creature-0-1-2-3-500-AA,\"Ula'tek\",0xa48,0x80000000";
const NOONE: &str = "0000000000000000,nil,0x80000000,0x80000000";
const PET: &str = "Pet-0-1-2-3-4-P,\"Pet\",0x1111,0x80000000";
const TOTEM: &str = "Creature-0-1-2-3-53006-SL,\"Spirit Link Totem\",0x2111,0x80000000";

fn aura(t: &str, ev: &str, src: &str, dst: &str, spell: &str, word: &str) -> String {
    at(t, &format!("{ev},{src},{dst},{spell},{word}"))
}

/// The buff rows keep what matters to the fight — an external with its
/// giver, a defensive and its stacks, an NPC's buff, the encounter's — and
/// nothing else: no long buff, no support buff; a defensive up before the
/// slice opens at t 0 (R18's way), and one removed twice closes once; the
/// debuff rows gain a player's debuffs on an NPC, never a pet's, never a
/// player's on a player.
#[test]
fn buffs_that_matter_and_players_debuffs_on_npcs() {
    let wall = "871,\"Shield Wall\",0x1";
    let supp = "33206,\"Pain Suppression\",0x2";
    let rake = "7201,\"Thorn Rake\",0x1";
    let seeds = [
        // Shield Wall up before the pull: R18's way, it opens at t 0.
        aura("00.000", "SPELL_AURA_APPLIED", TANK, TANK, wall, "BUFF"),
        at(
            "01.000",
            "SPELL_SUMMON,Player-1-S,\"Shaman-Realm-US\",0x512,0x80000000,Creature-0-1-2-3-53006-SL,\"Spirit Link Totem\",0x2111,0x80000000,98008,\"Spirit Link Totem\",0x8",
        ),
    ];
    let slice = [
        at("10.000", "ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004"),
        aura(
            "10.500",
            "SPELL_AURA_APPLIED_DOSE",
            TANK,
            TANK,
            wall,
            "BUFF,2",
        ),
        aura("11.000", "SPELL_AURA_REMOVED", TANK, TANK, wall, "BUFF"),
        aura("12.000", "SPELL_AURA_APPLIED", HEALER, TANK, supp, "BUFF"),
        aura(
            "12.100",
            "SPELL_AURA_APPLIED",
            HEALER,
            TANK,
            "21562,\"Power Word: Fortitude\",0x2",
            "BUFF",
        ),
        aura(
            "12.200",
            "SPELL_AURA_APPLIED",
            EVOKER,
            TANK,
            "395152,\"Ebon Might\",0x4",
            "BUFF",
        ),
        aura(
            "12.300",
            "SPELL_AURA_APPLIED",
            BOSS,
            TANK,
            "7006,\"Venom Surge\",0x8",
            "BUFF",
        ),
        aura(
            "12.400",
            "SPELL_AURA_APPLIED",
            NOONE,
            HEALER,
            "7007,\"Toxic Ward\",0x8",
            "BUFF",
        ),
        aura(
            "12.500",
            "SPELL_AURA_APPLIED",
            TOTEM,
            TANK,
            "325174,\"Spirit Link Totem\",0x8",
            "BUFF",
        ),
        aura(
            "12.600",
            "SPELL_AURA_APPLIED",
            TOTEM,
            TANK,
            "7008,\"Totem Ward\",0x8",
            "BUFF",
        ),
        aura("13.000", "SPELL_AURA_APPLIED", TANK, TANK, wall, "BUFF"),
        aura(
            "13.500",
            "SPELL_AURA_APPLIED_DOSE",
            TANK,
            TANK,
            wall,
            "BUFF,2",
        ),
        aura("14.000", "SPELL_AURA_APPLIED", HEALER, BOSS, rake, "DEBUFF"),
        aura(
            "14.500",
            "SPELL_AURA_APPLIED_DOSE",
            HEALER,
            BOSS,
            rake,
            "DEBUFF,3",
        ),
        aura(
            "14.600",
            "SPELL_AURA_APPLIED",
            HEALER,
            TANK,
            "7202,\"Mark\",0x1",
            "DEBUFF",
        ),
        aura(
            "14.700",
            "SPELL_AURA_APPLIED",
            PET,
            BOSS,
            "7203,\"Claw Bleed\",0x1",
            "DEBUFF",
        ),
        aura(
            "14.800",
            "SPELL_AURA_APPLIED",
            HEALER,
            BOSS,
            "774,\"Rejuvenation\",0x8",
            "BUFF",
        ),
        aura("15.000", "SPELL_AURA_REMOVED", HEALER, TANK, supp, "BUFF"),
        aura("15.500", "SPELL_AURA_REMOVED", HEALER, BOSS, rake, "DEBUFF"),
        // Removed twice: the second finds nothing applied.
        aura("16.000", "SPELL_AURA_REMOVED", HEALER, TANK, supp, "BUFF"),
        aura("17.000", "SPELL_AURA_REMOVED", TANK, TANK, wall, "BUFF"),
        at("20.000", "ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,10000"),
    ];
    let c = cut(
        seeds.iter().map(String::as_str),
        slice.iter().map(String::as_str),
        &Table,
        None,
    );
    let name = |u: Option<UnitId>| u.map_or("-", |u| c.units[u as usize].name.as_str());
    // Time, kind, whom, spell, by whom, stacks.
    type Row<'a> = (u32, EventKind, &'a str, u32, &'a str, Option<u16>);
    let rows: Vec<Row> = c
        .events
        .iter()
        .filter(|e| !e.kind.is_boss())
        .map(|e| {
            (
                e.t_ms,
                e.kind,
                name(e.unit),
                e.spell_id,
                name(e.src),
                e.stacks,
            )
        })
        .collect();
    use EventKind::*;
    let (tank, healer, boss, totem) = (
        "Tank-Realm-US",
        "Healer-Realm-US",
        "Ula'tek",
        "Spirit Link Totem",
    );
    assert_eq!(
        rows,
        [
            (0, BuffApplied, tank, 871, tank, None),
            (500, BuffDose, tank, 871, tank, Some(2)),
            (1000, BuffRemoved, tank, 871, tank, None),
            (2000, BuffApplied, tank, 33_206, healer, None),
            (2300, BuffApplied, tank, 7006, boss, None),
            (2400, BuffApplied, healer, 7007, "-", None),
            (2500, BuffApplied, tank, 325_174, totem, None),
            (3000, BuffApplied, tank, 871, tank, None),
            (3500, BuffDose, tank, 871, tank, Some(2)),
            (4000, DebuffApplied, boss, 7201, healer, None),
            (4500, DebuffDose, boss, 7201, healer, Some(3)),
            (5000, BuffRemoved, tank, 33_206, healer, None),
            (5500, DebuffRemoved, boss, 7201, healer, None),
            (7000, BuffRemoved, tank, 871, tank, None),
        ]
    );
    // The evoker, named by no row, is no unit; the pet neither.
    assert!(
        c.units
            .iter()
            .all(|u| u.name != "Evoker-Realm-US" && u.name != "Pet")
    );
}

/// A post keeps the block's shields and the last reading of every pool
/// kind its unit's posts have named: a warlock's shards stay after a line
/// naming his mana, and a rogue's `a|b` line names energy and combo points.
#[test]
fn a_post_keeps_its_shields_and_every_pool_kind() {
    let block = |guid: &str, absorb: u32, pool: &str| {
        format!(
            "{guid},0000000000000000,900,1000,0,0,0,0,0,{absorb},{pool},0,1.00,2.00,2434,0.0000,600"
        )
    };
    let lock = "Player-1-W,\"Lock-Realm-US\",0x512,0x80000000";
    let rogue = "Player-1-R,\"Rogue-Realm-US\",0x512,0x80000000";
    let cast = |t: &str, who: &str, guid: &str, absorb, pool: &str| {
        at(
            t,
            &format!(
                "SPELL_CAST_SUCCESS,{who},{BOSS},686,\"Bolt\",0x20,{}",
                block(guid, absorb, pool)
            ),
        )
    };
    let slice = [
        at("10.000", "ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004"),
        cast("11.000", lock, "Player-1-W", 250, "0,90,100"),
        cast("12.000", lock, "Player-1-W", 0, "7,2,5"),
        cast("13.000", lock, "Player-1-W", 0, "0,80,100"),
        cast("14.000", rogue, "Player-1-R", 40, "3|4,50|2,100|5"),
        cast("15.000", rogue, "Player-1-R", 40, "3,40,100"),
        at("20.000", "ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,10000"),
    ];
    let c = cut([], slice.iter().map(String::as_str), &Table, None);
    let w = |kind, current, max| Power { kind, current, max };
    let track = |name: &str| {
        let u = c.units.iter().find(|u| u.name == name).unwrap();
        u.posts
            .iter()
            .map(|p| (p.absorb, p.power, p.pools.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        track("Lock-Realm-US"),
        [
            (250, Some(w(0, 90, 100)), vec![w(0, 90, 100)]),
            (0, Some(w(7, 2, 5)), vec![w(0, 90, 100), w(7, 2, 5)]),
            (0, Some(w(0, 80, 100)), vec![w(0, 80, 100), w(7, 2, 5)]),
        ]
    );
    assert_eq!(
        track("Rogue-Realm-US"),
        [
            (40, Some(w(3, 50, 100)), vec![w(3, 50, 100), w(4, 2, 5)]),
            (40, Some(w(3, 40, 100)), vec![w(3, 40, 100), w(4, 2, 5)]),
        ]
    );
}

/// Format 5: a pool reads as the game's bar. A cast's success after its
/// cost (its block reports the caster before the cast pays); an energize's
/// gain on the pool it names, the target's block naming another — a
/// warlock's shards in the block's tenths (`1.0000` is 10), capped at the
/// suffix's max, full after a gain that lost any of itself to the cap, a
/// periodic energize alike; a paladin's holy power from its first gain,
/// against the last reading's max when the suffix writes none; a block
/// naming the energized pool already reads after the gain; no max known,
/// no reading.
#[test]
fn a_pool_reads_after_its_cost_and_rises_with_its_gains() {
    let lock = "Player-1-W,\"Lock-Realm-US\",0x512,0x80000000";
    let pal = "Player-1-P,\"Pal-Realm-US\",0x512,0x80000000";
    let dh = "Player-1-D,\"Havoc-Realm-US\",0x512,0x80000000";
    let other = "Player-1-Q,\"Other-Realm-US\",0x512,0x80000000";
    let guid = |who: &str| who.split(',').next().unwrap_or_default().to_string();
    let block = |who: &str, pool: &str| {
        format!(
            "{},0000000000000000,900,1000,0,0,0,0,0,0,{pool},1.00,2.00,2434,0.0000,600",
            guid(who)
        )
    };
    let cast = |t: &str, who: &str, spell: &str, pool: &str| {
        at(
            t,
            &format!(
                "SPELL_CAST_SUCCESS,{who},{BOSS},{spell},0x20,{}",
                block(who, pool)
            ),
        )
    };
    let gain = |t: &str, ev: &str, who: &str, pool: &str, suffix: &str| {
        at(
            t,
            &format!(
                "{ev},{who},{who},264178,\"Demonbolt\",0x20,{},{suffix}",
                block(who, pool)
            ),
        )
    };
    let e = "SPELL_ENERGIZE";
    let slice = [
        at("10.000", "ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004"),
        gain("11.000", e, lock, "0,250000,250000,0", "0.1000,0.9000,7,50"),
        cast(
            "11.100",
            lock,
            "686,\"Shadow Bolt\"",
            "0,250000,250000,3750",
        ),
        cast("12.000", lock, "105174,\"Hand of Gul'dan\"", "7,50,50,30"),
        gain("13.000", e, lock, "0,247000,250000,0", "2.0000,0.0000,7,50"),
        cast("14.000", lock, "105174,\"Hand of Gul'dan\"", "7,40,50,30"),
        gain(
            "15.000",
            "SPELL_PERIODIC_ENERGIZE",
            lock,
            "0,248000,250000,0",
            "1.0000,0.0000,7,50",
        ),
        gain("16.000", e, lock, "0,249000,250000,0", "5.0000,0.0000,7,50"),
        gain("17.000", e, pal, "0,250000,250000,0", "1.0000,0.0000,9,5"),
        cast(
            "18.000",
            pal,
            "383328,\"Final Verdict\"",
            "9|0,5|244973,5|250000,3|25000",
        ),
        gain("18.500", e, pal, "0,220000,250000,0", "1.0000,0.0000,9,0"),
        gain("19.000", e, pal, "0,220500,250000,0", "0.0000,1.0000,9,5"),
        gain("20.000", e, dh, "17,10,120,0", "10.0000,0.0000,17,120"),
        gain("20.500", e, other, "0,250000,250000,0", "1.0000,0.0000,9,0"),
        at("30.000", "ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,20000"),
    ];
    let c = cut([], slice.iter().map(String::as_str), &Table, None);
    let w = |kind, current, max| Power { kind, current, max };
    let track = |name: &str| {
        let u = c.units.iter().find(|u| u.name == name).unwrap();
        u.posts
            .iter()
            .map(|p| (p.t_ms, p.power, p.pools.clone()))
            .collect::<Vec<_>>()
    };
    let mana = |current| w(0, current, 250_000);
    let shards = |current| w(7, current, 50);
    assert_eq!(
        track("Lock-Realm-US"),
        [
            (1000, Some(mana(250_000)), vec![mana(250_000), shards(50)]),
            (1100, Some(mana(246_250)), vec![mana(246_250), shards(50)]),
            (2000, Some(shards(20)), vec![mana(246_250), shards(20)]),
            (3000, Some(mana(247_000)), vec![mana(247_000), shards(40)]),
            (4000, Some(shards(10)), vec![mana(247_000), shards(10)]),
            (5000, Some(mana(248_000)), vec![mana(248_000), shards(20)]),
            (6000, Some(mana(249_000)), vec![mana(249_000), shards(50)]),
        ],
        "capped: full; 50 − 30; + 2.0 shards; 40 − 30; + 1.0; + 5.0 to the max"
    );
    let holy = |current| w(9, current, 5);
    assert_eq!(
        track("Pal-Realm-US"),
        [
            (7000, Some(mana(250_000)), vec![mana(250_000), holy(1)]),
            (8000, Some(holy(2)), vec![mana(219_973), holy(2)]),
            (8500, Some(mana(220_000)), vec![mana(220_000), holy(3)]),
            (9000, Some(mana(220_500)), vec![mana(220_500), holy(5)]),
        ],
        "from its first gain; each pool after its own cost; the last max; capped"
    );
    assert_eq!(
        track("Havoc-Realm-US"),
        [(10_000, Some(w(17, 10, 120)), vec![w(17, 10, 120)])],
        "the block names the fury it gained: read, never added twice"
    );
    assert_eq!(
        track("Other-Realm-US"),
        [(10_500, Some(mana(250_000)), vec![mana(250_000)])],
        "no max in the suffix and no reading before it: no pool"
    );
}

/// Format 5: an energize STARTS a pool no block named only when it is a
/// counted class resource (its max below `COUNTED_POOL_MAX`): a paladin's
/// holy power before his first spender opens; a Mana Spring on an elemental
/// shaman, whose blocks name maelstrom, opens no mana pool, capped or not,
/// nor does an energy gain of max 100 on a druid in caster form; a druid's
/// mana a block has named still advances while his cat form's blocks name
/// energy.
#[test]
fn an_energize_starts_only_a_counted_pool() {
    let pal = "Player-1-P,\"Pal-Realm-US\",0x512,0x80000000";
    let sham = "Player-1-S,\"Sham-Realm-US\",0x512,0x80000000";
    let druid = "Player-1-D,\"Druid-Realm-US\",0x512,0x80000000";
    let line = |t: &str, ev: &str, who: &str, pool: &str, suffix: &str| {
        let guid = who.split(',').next().unwrap_or_default();
        at(
            t,
            &format!(
                "{ev},{who},{who},5394,\"Mana Spring\",0x8,{guid},0000000000000000,900,1000,0,0,0,0,0,0,{pool},1.00,2.00,2434,0.0000,600{suffix}"
            ),
        )
    };
    let e = "SPELL_ENERGIZE";
    let slice = [
        at("10.000", "ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004"),
        line("11.000", e, pal, "0,250000,250000,0", ",1.0000,0.0000,9,5"),
        line(
            "12.000",
            e,
            sham,
            "11,50,150,0",
            ",450.0000,0.0000,0,250000",
        ),
        line(
            "13.000",
            e,
            sham,
            "11,60,150,0",
            ",0.0000,675.0000,0,250000",
        ),
        line(
            "14.000",
            e,
            druid,
            "0,200000,250000,0",
            ",20.0000,0.0000,3,100",
        ),
        line("15.000", "SPELL_CAST_SUCCESS", druid, "3,100,100,0", ""),
        line(
            "16.000",
            e,
            druid,
            "3,90,100,0",
            ",450.0000,0.0000,0,250000",
        ),
        at("30.000", "ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,20000"),
    ];
    let c = cut([], slice.iter().map(String::as_str), &Table, None);
    let w = |kind, current, max| Power { kind, current, max };
    let pools = |name: &str| {
        let u = c.units.iter().find(|u| u.name == name).unwrap();
        u.posts.iter().map(|p| p.pools.clone()).collect::<Vec<_>>()
    };
    assert_eq!(
        pools("Pal-Realm-US"),
        [vec![w(0, 250_000, 250_000), w(9, 1, 5)]],
        "holy power, max 5, opens from its first gain"
    );
    assert_eq!(
        pools("Sham-Realm-US"),
        [vec![w(11, 50, 150)], vec![w(11, 60, 150)]],
        "mana no block named: no pool, capped or not"
    );
    assert_eq!(
        pools("Druid-Realm-US"),
        [
            vec![w(0, 200_000, 250_000)],
            vec![w(0, 200_000, 250_000), w(3, 100, 100)],
            vec![w(0, 200_450, 250_000), w(3, 90, 100)],
        ],
        "energy at max 100 is continuous: no pool from a gain; the mana a block named advances"
    );
}

/// R18's segment-start rule in the cut: a buff R18 names, or a player's
/// debuff on an NPC, first seen by a refresh, a dose or a removal was up
/// before the slice — an applied row at t 0 opens it, once per key, sorted
/// after the rows the first line made. An NPC's buff R18 does not name, and
/// a debuff on a player, keep their old rules.
#[test]
fn an_aura_up_before_the_cut_opens_at_its_start() {
    let ironfur = "192081,\"Ironfur\",0x1";
    let rake = "7201,\"Thorn Rake\",0x1";
    let slice = [
        at("10.000", "ENCOUNTER_START,3000,\"Ula'tek\",16,20,3004"),
        aura("11.000", "SPELL_AURA_REFRESH", TANK, TANK, ironfur, "BUFF"),
        aura("11.500", "SPELL_AURA_REFRESH", TANK, TANK, ironfur, "BUFF"),
        aura("12.000", "SPELL_AURA_REMOVED", TANK, TANK, ironfur, "BUFF"),
        // Once per key: a second removal with nothing up is no row.
        aura("12.500", "SPELL_AURA_REMOVED", TANK, TANK, ironfur, "BUFF"),
        aura(
            "13.000",
            "SPELL_AURA_REMOVED_DOSE",
            HEALER,
            BOSS,
            rake,
            "DEBUFF,1",
        ),
        aura("13.500", "SPELL_AURA_REMOVED", HEALER, BOSS, rake, "DEBUFF"),
        // An NPC's buff R18 does not name: no application, no row.
        aura(
            "14.000",
            "SPELL_AURA_REMOVED",
            BOSS,
            TANK,
            "7006,\"Venom Surge\",0x8",
            "BUFF",
        ),
        // A debuff on a player keeps every row, as it always did.
        aura(
            "14.500",
            "SPELL_AURA_REMOVED",
            BOSS,
            TANK,
            "7003,\"Rot\",0x8",
            "DEBUFF",
        ),
        // An external first seen by its refresh: open from t 0 to the end.
        aura(
            "15.000",
            "SPELL_AURA_REFRESH",
            HEALER,
            TANK,
            "33206,\"Pain Suppression\",0x2",
            "BUFF",
        ),
        at("20.000", "ENCOUNTER_END,3000,\"Ula'tek\",16,20,1,10000"),
    ];
    let c = cut([], slice.iter().map(String::as_str), &Table, None);
    let name = |u: Option<UnitId>| u.map_or("-", |u| c.units[u as usize].name.as_str());
    // Time, kind, whom, spell, by whom, stacks.
    type Row<'a> = (u32, EventKind, &'a str, u32, &'a str, Option<u16>);
    let rows: Vec<Row> = c
        .events
        .iter()
        .map(|e| {
            (
                e.t_ms,
                e.kind,
                name(e.unit),
                e.spell_id,
                name(e.src),
                e.stacks,
            )
        })
        .collect();
    use EventKind::*;
    let (tank, healer, boss) = ("Tank-Realm-US", "Healer-Realm-US", "Ula'tek");
    assert_eq!(
        rows,
        [
            (0, BossEngaged, "-", 3000, "-", None),
            (0, BuffApplied, tank, 192_081, tank, None),
            (0, DebuffApplied, boss, 7201, healer, None),
            (0, BuffApplied, tank, 33_206, healer, None),
            (2000, BuffRemoved, tank, 192_081, tank, None),
            (3000, DebuffDose, boss, 7201, healer, Some(1)),
            (3500, DebuffRemoved, boss, 7201, healer, None),
            (4500, DebuffRemoved, tank, 7003, boss, None),
            (10_000, BossKilled, "-", 3000, "-", None),
        ]
    );
    // Units numbered where their lines name them: the tank (Ironfur's
    // refresh), then the boss and the healer (the rake's dose).
    let units: Vec<&str> = c.units.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(units, [tank, boss, healer]);
}
