use super::*;

/// An event as the tests compare it: time, kind, unit, spell, src, target.
type EventRow = (u32, EventKind, UnitId, u32, Option<UnitId>, Option<UnitId>);
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
    // The same player on another floor once: not posted, and a hit there
    // is no row.
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
fn posts_are_the_floors_with_health_and_power() {
    let c = second();
    let p = |t_ms, x, y, facing, hp, power: Option<(u32, u64, u64)>| Post {
        t_ms,
        x,
        y,
        facing,
        hp,
        power: power.map(|(kind, current, max)| Power { kind, current, max }),
    };
    assert_eq!(
        c.units[0].posts,
        [
            p(500, 150_025, -350, 15_000, 900, Some((0, 100, 100))),
            p(1000, 150_100, -300, 16_000, 800, Some((0, 100, 100))),
        ],
        "the post on floor 2435 is not kept"
    );
    assert_eq!(
        c.units[1].posts,
        [p(2500, 151_000, 0, 31_416, 900, Some((3, 25, 100)))]
    );
    // No pool, no power (the text cutter wrote `1,0,0`).
    assert_eq!(c.units[2].posts, [p(3000, 152_000, 500, 0, 1000, None)]);
    assert_eq!(c.posts(), 5);
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
            (500, PcastSuccess, 0, 49_998, None, Some(1)),
            (1000, Hit, 0, 7000, Some(1), None),
            (2000, CastStart, 1, 7001, None, None),
            (2500, CastSuccess, 1, 7001, None, Some(0)),
            (3000, CastSuccess, 2, 7002, None, None),
            (5000, DebuffApplied, 0, 7003, Some(1), None),
            (5500, DebuffDose, 0, 7003, Some(1), None),
            (6500, PcastFailed, 0, 49_998, None, None),
            (7000, DebuffRemoved, 0, 7003, Some(1), None),
            (8000, Hit, 0, 7000, Some(1), None),
            (9000, Interrupt, 1, 7001, Some(0), None),
            (10_000, NpcDied, 2, 0, None, None),
            (11_000, Death, 0, 0, None, None),
            (11_500, Rez, 0, 20_484, Some(4), None),
        ]
    );
    let hit = &c.events[1];
    assert_eq!(
        (hit.at, hit.base, hit.spell.as_str()),
        (Some((150_100, -300)), Some(120), "Venom")
    );
    let miss = &c.events[9];
    assert_eq!((miss.at, miss.base), (None, None));
    assert_eq!(c.events[6].stacks, Some(2));
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
    assert_eq!(kinds, [EventKind::NpcDied]);
    assert_eq!(c.head.success, Some(false));
}
