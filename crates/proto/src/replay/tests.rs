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

fn post(t_ms: u32, x: i32, y: i32, facing: i32, hp: u16, power: Option<(u32, u64, u64)>) -> Post {
    Post {
        t_ms,
        x,
        y,
        facing,
        hp,
        power: power.map(|(kind, current, max)| Power { kind, current, max }),
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
        unit,
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
/// read byte for byte as that test expects, so a pull exported from the
/// store and one cut by the extractor read alike.
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
        "unit,t_ms,x,y,facing,hp,power_type,power,power_max\n\
         0,500,1500.25,-3.50,1.500,90.0,0,100,100\n\
         0,1000,1501.00,-3.00,1.600,80.0,0,100,100\n\
         1,2500,1510.00,0.00,3.142,90.0,3,25,100\n\
         2,3000,1520.00,5.00,0.000,100.0,1,0,0\n\
         3,4000,1499.00,-2.00,0.500,100.0,0,0,0\n"
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
