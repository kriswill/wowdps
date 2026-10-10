use super::csv::{self, PullFacts};
use super::*;

fn unit(kind: UnitKind, name: &str, npc: u32, posts: Vec<Post>) -> Unit {
    Unit {
        kind,
        name: name.into(),
        guid: String::new(),
        class: None,
        spec: None,
        you: false,
        npc,
        posts,
    }
}

/// A post on `ulatek()`'s floor, 2434.
fn post(t_ms: u32, x: i32, y: i32, facing: i32, hp: u16, power: Option<(u32, u64, u64)>) -> Post {
    Post {
        t_ms,
        x,
        y,
        facing,
        hp,
        power: power.map(|(kind, current, max)| Power { kind, current, max }),
        map_id: 2434,
    }
}

/// `p` on another floor.
fn on(map_id: u32, p: Post) -> Post {
    Post { map_id, ..p }
}

/// A boss row: the encounter as its spell, no unit.
fn boss_row(t_ms: u32, kind: EventKind, id: u32, name: &str) -> Event {
    Event {
        t_ms,
        kind,
        unit: None,
        spell_id: id,
        spell: name.into(),
        at: None,
        src: None,
        stacks: None,
        base: None,
        target: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn event(
    t_ms: u32,
    kind: EventKind,
    unit: u32,
    spell_id: u32,
    spell: &str,
    at: Option<(i32, i32)>,
    src: Option<u32>,
    stacks: Option<u16>,
    base: Option<u64>,
    target: Option<u32>,
) -> Event {
    Event {
        t_ms,
        kind,
        unit: Some(unit),
        spell_id,
        spell: spell.into(),
        at,
        src,
        stacks,
        base,
        target,
    }
}

/// The second pull of the extractor's own cutter test (`pullcut::tests`
/// on the replay branch), as its writers wrote it: every file here must
/// read byte for byte as that test expects (but for `tracks.csv`'s
/// trailing `map_id`, format 2's), so a pull exported from the store and
/// one cut by the extractor read alike.
fn ulatek() -> Cut {
    use EventKind::*;
    let mut tank = unit(
        UnitKind::Player,
        "Tank-Realm-US",
        0,
        vec![
            post(500, 150_025, -350, 15_000, 900, Some((0, 100, 100))),
            post(1000, 150_100, -300, 16_000, 800, Some((0, 100, 100))),
        ],
    );
    tank.guid = "Player-1-A".into();
    tank.class = Some(Class::DeathKnight);
    tank.spec = Some(Spec::Blood);
    tank.you = true;
    let mut healer = unit(UnitKind::Player, "Unknown", 0, Vec::new());
    healer.guid = "Player-1-B".into();
    Cut {
        head: Head {
            encounter: Some(EncounterHead {
                id: 3000,
                name: "Ula'tek".into(),
                difficulty: 16,
                size: 20,
            }),
            key: None,
            map: 3004,
            zone: "The Venomous Abyss".into(),
            success: Some(true),
            fight_ms: Some(12_000),
            start_utc_ms: 1_791_428_460_000,
            date: (2026, 10, 7),
        },
        floor: 2434,
        units: vec![
            tank,
            unit(
                UnitKind::Boss,
                "Ula'tek",
                500,
                vec![post(2500, 151_000, 0, 31_416, 900, Some((3, 25, 100)))],
            ),
            unit(
                UnitKind::Add,
                "Egg",
                501,
                vec![post(3000, 152_000, 500, 0, 1000, Some((1, 0, 0)))],
            ),
            unit(
                UnitKind::Friendly,
                "Healing Stream Totem",
                5394,
                vec![post(4000, 149_900, -200, 5000, 1000, Some((0, 0, 0)))],
            ),
            healer,
        ],
        events: vec![
            event(
                500,
                PcastSuccess,
                0,
                49_998,
                "Death Strike",
                None,
                None,
                None,
                None,
                Some(1),
            ),
            event(
                1000,
                Hit,
                0,
                7000,
                "Venom",
                Some((150_100, -300)),
                Some(1),
                None,
                Some(120),
                None,
            ),
            event(
                2000, CastStart, 1, 7001, "Spit", None, None, None, None, None,
            ),
            event(
                2500,
                CastSuccess,
                1,
                7001,
                "Spit",
                None,
                None,
                None,
                None,
                Some(0),
            ),
            event(
                3000,
                CastSuccess,
                2,
                7002,
                "Hatch",
                None,
                None,
                None,
                None,
                None,
            ),
            event(
                5000,
                DebuffApplied,
                0,
                7003,
                "Rot",
                None,
                Some(1),
                None,
                None,
                None,
            ),
            event(
                5500,
                DebuffDose,
                0,
                7003,
                "Rot",
                None,
                Some(1),
                Some(2),
                None,
                None,
            ),
            event(
                7000,
                DebuffRemoved,
                0,
                7003,
                "Rot",
                None,
                Some(1),
                None,
                None,
                None,
            ),
            event(8000, Hit, 0, 7000, "Venom", None, Some(1), None, None, None),
            event(
                9000,
                Interrupt,
                1,
                7001,
                "Spit",
                None,
                Some(0),
                None,
                None,
                None,
            ),
            event(10_000, NpcDied, 2, 0, "", None, None, None, None, None),
            event(11_000, Death, 0, 0, "", None, None, None, None, None),
            event(
                11_500,
                Rez,
                0,
                20_484,
                "Rebirth",
                None,
                Some(4),
                None,
                None,
                None,
            ),
        ],
        placed: vec![
            Placed {
                t_ms: 1000,
                kind: PlacedKind::Cast,
                unit: Some(0),
                spell_id: 111_771,
                spell: "Demonic Gateway".into(),
                at: Some((10_000, 20_000)),
                src: None,
                target: None,
            },
            Placed {
                t_ms: 3000,
                kind: PlacedKind::Summon,
                unit: Some(2),
                spell_id: 192_077,
                spell: "Wind Rush Totem".into(),
                at: None,
                src: None,
                target: Some(3),
            },
            Placed {
                t_ms: 5000,
                kind: PlacedKind::Touch,
                unit: Some(0),
                spell_id: 145_629,
                spell: "Anti-Magic Zone".into(),
                at: None,
                src: None,
                target: None,
            },
            Placed {
                t_ms: 6000,
                kind: PlacedKind::Touch,
                unit: Some(0),
                spell_id: 73_921,
                spell: "Healing Rain".into(),
                at: Some((9550, 19_525)),
                src: Some(2),
                target: None,
            },
            Placed {
                t_ms: 7000,
                kind: PlacedKind::Gone,
                unit: Some(3),
                spell_id: 0,
                spell: String::new(),
                at: None,
                src: None,
                target: None,
            },
        ],
        markers: vec![
            Marker {
                t_ms: 0,
                kind: MarkerKind::Placed,
                marker: 1,
                at: Some((9000, -1000)),
            },
            Marker {
                t_ms: 0,
                kind: MarkerKind::Placed,
                marker: 4,
                at: Some((10_050, -2025)),
            },
            Marker {
                t_ms: 6000,
                kind: MarkerKind::Removed,
                marker: 4,
                at: None,
            },
            Marker {
                t_ms: 6250,
                kind: MarkerKind::Placed,
                marker: 7,
                at: Some((11_000, -3000)),
            },
        ],
    }
}

#[test]
fn a_cut_round_trips() {
    let cut = ulatek();
    let bytes = encode(&cut);
    assert_eq!(format_of(&bytes), Some(FORMAT));
    assert_eq!(decode(&bytes), Some(cut));
    let empty = Cut::default();
    assert_eq!(decode(&encode(&empty)), Some(empty));
}

/// Posts against their unit's last: the pool coming and going, a
/// backward clock, big moves, every field at once.
#[test]
fn posts_round_trip_through_every_change() {
    let mut cut = ulatek();
    cut.units[0].posts = vec![
        post(0, 0, 0, 0, 0, None),
        post(0, 0, 0, 0, 0, None),
        post(6, -5, 7, 62_831, 1000, Some((0, 250_000, 250_000))),
        post(7, -5, 7, 62_831, 1000, Some((0, 240_000, 250_000))),
        post(5, i32::MAX, i32::MIN, -1, 6553, Some((3, 0, 100))),
        post(u32::MAX, 0, 0, 0, 0, None),
        post(100, 1, 1, 1, 1, Some((17, 120, 120))),
        post(100, 1, 1, 1, 1, Some((17, 0, 120))),
    ];
    assert_eq!(decode(&encode(&cut)), Some(cut));
}

/// Truncation, a wrong format, a flipped byte: `None`, never a panic.
#[test]
fn bad_bytes_are_refused_never_a_panic() {
    let bytes = encode(&ulatek());
    for n in 0..bytes.len() {
        assert_eq!(decode(&bytes[..n]), None, "cut at {n}");
    }
    let mut other = bytes.clone();
    other[4] = FORMAT + 1;
    assert_eq!(decode(&other), None);
    assert_eq!(format_of(&other), Some(FORMAT + 1));
    assert_eq!(format_of(b"WDSR\x01\0\0\0\0"), None, "a series head");
    for i in 0..bytes.len() {
        for flip in [0xff, 0x01, 0x80] {
            let mut b = bytes.clone();
            b[i] ^= flip;
            let _ = decode(&b);
        }
    }
    // Format 2's own: a track leaving its floor, the boss rows — and the
    // format-1 golden, every byte of it flipped too.
    for bytes in [encode(&small_floors()), unhex(FORMAT1)] {
        for n in 0..bytes.len() {
            assert_eq!(decode(&bytes[..n]), None, "cut at {n}");
        }
        for i in 0..bytes.len() {
            for flip in [0xff, 0x01, 0x80, 0x20] {
                let mut b = bytes.clone();
                b[i] ^= flip;
                let _ = decode(&b);
            }
        }
    }
    // A count that lies about what follows is refused before allocating.
    let mut liar = Vec::from(&MAGIC[..]);
    liar.push(FORMAT);
    liar.extend_from_slice(&3u32.to_le_bytes());
    liar.extend_from_slice(&[0xff, 0xff, 0x7f]);
    assert_eq!(decode(&liar), None);
}

#[test]
fn a_long_pull_is_a_few_bytes_a_post() {
    // Ten minutes of a unit posted every 200 ms, walking and turning.
    let posts: Vec<Post> = (0..3000u32)
        .map(|i| {
            let i32_of = |v: u32| i32::try_from(v).unwrap();
            post(
                i * 200,
                150_000 + i32_of(i % 50) * 30,
                i32_of(i / 50) * 10,
                i32_of(i % 7) * 1000,
                u16::try_from(1000 - i / 3).unwrap(),
                Some((0, u64::from(250_000 - i * 10), 250_000)),
            )
        })
        .collect();
    let mut cut = ulatek();
    cut.units[0].posts = posts;
    let bytes = encode(&cut).len();
    assert!(bytes < 3000 * 9, "{bytes} bytes for 3000 posts");
}

#[test]
fn the_files_read_as_the_extractor_wrote_them() {
    let cut = ulatek();
    assert_eq!(
        csv::tracks_csv(&cut),
        "unit,t_ms,x,y,facing,hp,power_type,power,power_max,map_id\n\
         0,500,1500.25,-3.50,1.500,90.0,0,100,100,2434\n\
         0,1000,1501.00,-3.00,1.600,80.0,0,100,100,2434\n\
         1,2500,1510.00,0.00,3.142,90.0,3,25,100,2434\n\
         2,3000,1520.00,5.00,0.000,100.0,1,0,0,2434\n\
         3,4000,1499.00,-2.00,0.500,100.0,0,0,0,2434\n"
    );
    assert_eq!(
        csv::events_csv(&cut),
        "t_ms,kind,unit,spell_id,spell,x,y,src,stacks,base,target\n\
         500,pcast_success,0,49998,Death Strike,,,,,,1\n\
         1000,hit,0,7000,Venom,1501.00,-3.00,1,,120\n\
         2000,cast_start,1,7001,Spit,,,,,,\n\
         2500,cast_success,1,7001,Spit,,,,,,0\n\
         3000,cast_success,2,7002,Hatch,,,,,,\n\
         5000,debuff_applied,0,7003,Rot,,,1\n\
         5500,debuff_dose,0,7003,Rot,,,1,2\n\
         7000,debuff_removed,0,7003,Rot,,,1\n\
         8000,hit,0,7000,Venom,,,1,,\n\
         9000,interrupt,1,7001,Spit,,,0\n\
         10000,npc_died,2,,,,,\n\
         11000,death,0,,,,,\n\
         11500,rez,0,20484,Rebirth,,,4\n"
    );
    assert_eq!(
        csv::units_tsv(&cut),
        "unit\tkind\tname\tclass\tspec\tspec_name\trole\tyou\tnpc\n\
         0\tplayer\tTank-Realm-US\tDeathKnight\t250\tBlood\ttank\t1\t\n\
         1\tboss\tUla'tek\t\t\t\t\t0\t500\n\
         2\tadd\tEgg\t\t\t\t\t0\t501\n\
         3\tfriendly\tHealing Stream Totem\t\t\t\t\t0\t5394\n\
         4\tplayer\tUnknown\t\t\t\t\t0\t\n"
    );
    assert_eq!(
        csv::placed_csv(&cut),
        "t_ms,kind,unit,spell_id,spell,x,y,src,target\n\
         1000,cast,0,111771,Demonic Gateway,100.00,200.00,,\n\
         3000,summon,2,192077,Wind Rush Totem,,,,3\n\
         5000,touch,0,145629,Anti-Magic Zone,,,,\n\
         6000,touch,0,73921,Healing Rain,95.50,195.25,2,\n\
         7000,gone,3,,,,,,\n"
    );
    assert_eq!(
        csv::markers_csv(&cut),
        "t_ms,kind,marker,x,y\n0,placed,1,90.00,-10.00\n0,placed,4,100.50,-20.25\n\
         6000,removed,4,,\n6250,placed,7,110.00,-30.00\n"
    );
    assert_eq!(
        csv::raid_csv(&[1000, 500, 0, 1800], 1000),
        "t_ms,damage\n0,1000\n1000,500\n2000,0\n3000,1800\n"
    );
    let facts = PullFacts {
        instance: None,
        log: "WoWCombatLog-100726_200000.txt",
        owner: Some("Player-1-A"),
        order: Some(3),
    };
    assert_eq!(
        csv::pull_txt(&cut, &facts),
        "title = Ula'tek\ndetail = Mythic, 20 players\noutcome = Kill  0:12\n\
         instance = The Venomous Abyss\nkind = raid\nencounter_id = 3000\nmap = 3004\n\
         date = 2026-10-07\nlog = WoWCombatLog-100726_200000.txt\nstart = 1791428460000\n\
         owner = Player-1-A\norder = 3\n"
    );
    let named = PullFacts {
        instance: Some("March on Quel'Danas"),
        ..facts
    };
    assert!(csv::pull_txt(&cut, &named).contains("\ninstance = March on Quel'Danas\n"));
}

#[test]
fn a_keys_run_names_its_dungeon_and_level() {
    let mut cut = ulatek();
    cut.head.encounter = None;
    cut.head.key = Some(KeyHead {
        name: "Kings' Rest".into(),
        level: 15,
    });
    cut.head.fight_ms = Some(1_603_290);
    let text = csv::pull_txt(&cut, &PullFacts::default());
    assert!(
        text.starts_with(
            "title = Kings' Rest +15\ndetail = Mythic+ 15, 2 players\noutcome = Timed  26:43\n"
        ),
        "{text}"
    );
    assert!(!text.contains("encounter_id"));
}

#[test]
fn a_name_is_quoted_only_when_it_holds_a_comma() {
    assert_eq!(csv::csv_name("Hammer of Wrath"), "Hammer of Wrath");
    assert_eq!(
        csv::csv_name("First In, Last Out"),
        "\"First In, Last Out\""
    );
    assert_eq!(csv::csv_name("nil"), "nil");
}

#[test]
fn the_seven_files_are_written() {
    let dir = std::env::temp_dir().join(format!("wowdps-replay-csv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    csv::write_dir(
        &dir,
        &ulatek(),
        Some((&[5, 6], 1000)),
        &PullFacts::default(),
    )
    .unwrap();
    for name in csv::FILES {
        assert!(dir.join(name).is_file(), "{name}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("raid.csv")).unwrap(),
        "t_ms,damage\n0,5\n1000,6\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- format 2: every floor, the boss rows ------------------------------------------

/// A small pull: a tank posted twice, the boss once, a hit, a death, a
/// gateway and a marker — what the format-1 golden bytes hold.
fn small() -> Cut {
    let mut tank = unit(
        UnitKind::Player,
        "Tank-Realm-US",
        0,
        vec![
            post(500, 150_025, -350, 15_000, 900, Some((6, 100, 100))),
            post(1000, 150_100, -300, 16_000, 800, Some((6, 100, 100))),
        ],
    );
    tank.guid = "Player-1-A".into();
    tank.class = Some(Class::DeathKnight);
    tank.spec = Some(Spec::Blood);
    tank.you = true;
    Cut {
        head: Head {
            encounter: Some(EncounterHead {
                id: 3000,
                name: "Ula'tek".into(),
                difficulty: 16,
                size: 20,
            }),
            key: None,
            map: 3004,
            zone: "The Venomous Abyss".into(),
            success: Some(true),
            fight_ms: Some(30_000),
            start_utc_ms: 1_791_428_460_000,
            date: (2026, 10, 7),
        },
        floor: 2434,
        units: vec![
            tank,
            unit(
                UnitKind::Boss,
                "Ula'tek",
                500,
                vec![post(2500, 151_000, 0, 31_416, 900, Some((3, 25, 100)))],
            ),
        ],
        events: vec![
            event(
                1000,
                EventKind::Hit,
                0,
                7000,
                "Venom",
                Some((150_100, -300)),
                Some(1),
                None,
                Some(120),
                None,
            ),
            event(
                11_000,
                EventKind::Death,
                0,
                0,
                "",
                None,
                None,
                None,
                None,
                None,
            ),
        ],
        placed: vec![Placed {
            t_ms: 1000,
            kind: PlacedKind::Cast,
            unit: Some(0),
            spell_id: 111_771,
            spell: "Demonic Gateway".into(),
            at: Some((10_000, 20_000)),
            src: None,
            target: None,
        }],
        markers: vec![Marker {
            t_ms: 0,
            kind: MarkerKind::Placed,
            marker: 1,
            at: Some((9000, -1000)),
        }],
    }
}

/// `small()` in format 2: the tank steps onto floor 2435 and back, the
/// boss rows open and close it.
fn small_floors() -> Cut {
    let mut cut = small();
    let tank = &mut cut.units[0].posts;
    tank.push(on(
        2435,
        post(1500, 1000, 2000, 0, 700, Some((6, 100, 100))),
    ));
    tank.push(post(2000, 150_100, -300, 0, 700, Some((6, 100, 100))));
    cut.events
        .insert(0, boss_row(0, EventKind::BossEngaged, 3000, "Ula'tek"));
    cut.events
        .push(boss_row(30_000, EventKind::BossKilled, 3000, "Ula'tek"));
    cut
}

fn unhex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `small()` as a format-1 build wrote it (v45, before every floor).
const FORMAT1: &str = "57445250011d0000000801004c024c1b036713047a2c05a6011506bb010d07c8010908d1010a07125468652056656e6f6d6f757320416279737307556c612774656b0d54616e6b2d5265616c6d2d55530a506c617965722d312d41000556656e6f6d0f44656d6f6e696320476174657761798213bc17000201b0ea01c0f7a797a368ea0f0a0701b817011014000200020306fa01010002010104000000f40301ffe80792a812bb05b0ea01880e010664c8013fe807960164d00fc701ff8827b0b71200f0ea03880e0103643202d00f0000000ba8a912d7040178a09c010a00010001d00f00030002a09c01c0b80201000101d08c01cf0f03d8360500049be90606";

/// `small_floors()` as this build writes it.
const FORMAT2: &str = "5744525002210000000901004c024c1b036713047a4105bb012106dc010d07e9010908f2010d09ff010c07125468652056656e6f6d6f757320416279737307556c612774656b0d54616e6b2d5265616c6d2d55530a506c617965722d312d41000556656e6f6d0f44656d6f6e696320476174657761798213bc17000201b0ea01c0f7a797a368ea0f0a0701b817011014000200020306fa01010004010104000000f40301ffe80792a812bb05b0ea01880e010664c8013fe807960164d00fc7013fe807d79912f823fff901c7010fe807d89912f723ff8827b0b71200f0ea03880e0103643204000d000020d00f0000010ba8a912d7040178a09c010a000200f0a8020e00002001d00f00030003a09c01c0b80201000101d08c01cf0f04b81701d8360500049be90606028213831301000202010100";

/// A format-1 file still reads: every post on its floor, every row naming
/// its unit, no boss rows.
#[test]
fn a_format_1_file_reads_every_post_on_its_floor() {
    let bytes = unhex(FORMAT1);
    assert_eq!(format_of(&bytes), Some(1));
    let cut = decode(&bytes).expect("format 1 decodes");
    assert_eq!(cut, small());
    assert!(
        cut.units
            .iter()
            .flat_map(|u| &u.posts)
            .all(|p| p.map_id == 2434)
    );
    let maps: Vec<(u32, usize, usize)> = cut
        .maps()
        .iter()
        .map(|m| (m.map_id, m.posts, m.players))
        .collect();
    assert_eq!(maps, [(2434, 3, 2)]);
}

/// Format 2's golden bytes: a diff here is a format change, never a
/// fixture to refresh.
#[test]
fn format_2_reads_and_writes_its_golden_bytes() {
    let cut = small_floors();
    let bytes = encode(&cut);
    assert_eq!(hex(&bytes), FORMAT2);
    assert_eq!(format_of(&bytes), Some(2));
    assert_eq!(decode(&unhex(FORMAT2)), Some(cut));
}

/// Tracks moving between floors, a unit never on the head's floor, a head
/// with no floor at all, rows naming no unit: all round-trip.
#[test]
fn every_floor_round_trips() {
    let mut cut = ulatek();
    cut.units[0].posts = vec![
        post(0, 0, 0, 0, 0, None),
        on(2435, post(10, 1, 1, 0, 0, None)),
        on(2435, post(20, 2, 2, 0, 0, None)),
        on(2433, post(30, 3, 3, 0, 0, None)),
        post(40, 4, 4, 0, 0, None),
        on(u32::MAX, post(50, 5, 5, 0, 0, None)),
    ];
    cut.units[2].posts = vec![on(7, post(3000, 1, 2, 3, 4, None))];
    cut.events
        .insert(0, boss_row(0, EventKind::BossEngaged, 3000, "Ula'tek"));
    cut.events
        .push(boss_row(12_000, EventKind::BossWiped, 3000, "Ula'tek"));
    assert_eq!(decode(&encode(&cut)), Some(cut.clone()));
    // No floor (no player posted): every track leaves it.
    cut.floor = 0;
    assert_eq!(decode(&encode(&cut)), Some(cut.clone()));
    // Boss rows in a cut with no unit at all.
    let empty = Cut {
        events: vec![boss_row(0, EventKind::BossEngaged, 1, "A")],
        ..Cut::default()
    };
    assert_eq!(decode(&encode(&empty)), Some(empty));
}

/// Where section `tag` lies in a file: (offset, length), off its index.
fn section_at(b: &[u8], tag: u8) -> (usize, usize) {
    let index_len = u32::from_le_bytes(b[5..9].try_into().unwrap()) as usize;
    let mut c = Cur::new(&b[HEAD_LEN..HEAD_LEN + index_len]);
    let n = c.usize().unwrap();
    for _ in 0..n {
        let (t, from, len) = (c.u8().unwrap(), c.usize().unwrap(), c.usize().unwrap());
        if t == tag {
            return (HEAD_LEN + index_len + from, len);
        }
    }
    panic!("no section {tag}");
}

/// A pull on one floor pays four bytes for its maps section: a one-floor
/// table (its count and 2434's two bytes) and no unit leaving it.
#[test]
fn one_floor_costs_four_bytes() {
    let bytes = encode(&ulatek());
    let (from, len) = section_at(&bytes, MAPS);
    assert_eq!(&bytes[from..from + len], &[1, 0x82, 0x13, 0]);
}

/// `small_floors()` with its maps section replaced by `maps`.
fn with_maps(maps: &[u8]) -> Option<Cut> {
    let good = encode(&small_floors());
    let (from, len) = section_at(&good, MAPS);
    // The maps section is written last, its length the index's last byte.
    assert_eq!(from + len, good.len());
    let mut b = good[..from].to_vec();
    b.extend_from_slice(maps);
    let index_len = u32::from_le_bytes(b[5..9].try_into().unwrap()) as usize;
    b[HEAD_LEN + index_len - 1] = u8::try_from(maps.len()).unwrap();
    decode(&b)
}

/// A format-1 head over format-2 rows is no file a format-1 build wrote
/// (a boss row's kind is past format 1's), and every lie the maps section
/// or a boss row can tell is refused.
#[test]
fn format_2_lies_are_refused() {
    let mut bytes = encode(&small_floors());
    bytes[4] = 1;
    assert_eq!(decode(&bytes), None, "format 1 knew no boss row");
    // The table 2434, 2435; the tank (unit 0) onto 2435 at post 2, back at 3.
    let good = [2, 0x82, 0x13, 0x83, 0x13, 1, 0, 2, 2, 1, 1, 0];
    assert_eq!(with_maps(&good), Some(small_floors()));
    for lie in [
        // The table out of order.
        &[2, 0x83, 0x13, 0x82, 0x13, 1, 0, 2, 2, 1, 1, 0][..],
        // A unit past the units.
        &[2, 0x82, 0x13, 0x83, 0x13, 1, 9, 1, 2, 1][..],
        // A unit listed twice.
        &[2, 0x82, 0x13, 0x83, 0x13, 2, 0, 1, 2, 1, 0, 1, 3, 0][..],
        // A change at no post.
        &[2, 0x82, 0x13, 0x83, 0x13, 1, 0, 1, 9, 1][..],
        // A change not after the last.
        &[2, 0x82, 0x13, 0x83, 0x13, 1, 0, 2, 2, 1, 0, 0][..],
        // A floor past the table.
        &[2, 0x82, 0x13, 0x83, 0x13, 1, 0, 1, 2, 5][..],
        // A unit listed with no change.
        &[2, 0x82, 0x13, 0x83, 0x13, 1, 0, 0, 0, 0][..],
        // A count the bytes cannot hold.
        &[2, 0x82, 0x13, 0x83, 0x13, 9, 0, 1, 2, 1][..],
        // Trailing bytes.
        &[2, 0x82, 0x13, 0x83, 0x13, 1, 0, 2, 2, 1, 1, 0, 0][..],
    ] {
        assert_eq!(with_maps(lie), None, "{lie:02x?}");
    }
    // The first event is the boss row: n, dt, kind 13, unit 0, spell, has.
    let good = encode(&small_floors());
    let (events, _) = section_at(&good, EVENTS);
    assert_eq!(good[events + 2..events + 4], [13, 0]);
    assert_eq!(good[events + 5], NO_UNIT);
    let mut b = good.clone();
    b[events + 3] = 1;
    assert_eq!(decode(&b), None, "no unit, yet a unit written");
    let mut b = good.clone();
    b[events + 5] = NO_UNIT | 1 << 6;
    assert_eq!(decode(&b), None, "a flag past bit 5");
    let mut b = good;
    b[events + 2] = u8::try_from(EventKind::ALL.len()).unwrap();
    assert_eq!(decode(&b), None, "a kind past the kinds");
}

#[test]
fn the_files_carry_every_floor_and_the_boss_rows() {
    let cut = small_floors();
    assert_eq!(
        csv::tracks_csv(&cut),
        "unit,t_ms,x,y,facing,hp,power_type,power,power_max,map_id\n\
         0,500,1500.25,-3.50,1.500,90.0,6,100,100,2434\n\
         0,1000,1501.00,-3.00,1.600,80.0,6,100,100,2434\n\
         0,1500,10.00,20.00,0.000,70.0,6,100,100,2435\n\
         0,2000,1501.00,-3.00,0.000,70.0,6,100,100,2434\n\
         1,2500,1510.00,0.00,3.142,90.0,3,25,100,2434\n"
    );
    assert_eq!(
        csv::events_csv(&cut),
        "t_ms,kind,unit,spell_id,spell,x,y,src,stacks,base,target\n\
         0,boss_engaged,,3000,Ula'tek,,,\n\
         1000,hit,0,7000,Venom,1501.00,-3.00,1,,120\n\
         11000,death,0,,,,,\n\
         30000,boss_killed,,3000,Ula'tek,,,\n"
    );
}
