//! R29 (v45): the replay cut over every gated fixture, against the golden
//! TSVs `fixtures/check.awk` computes from the log grammar alone — the
//! floor, the posts (per floor, per player and in all), the units
//! numbered, every event kind (the boss rows too), every placed kind and
//! the world markers of each closed boss pull and each finished keystone
//! run — and the cut from a lazily loaded segment (its index seeds) equal
//! to the cut from every line before it (a full replay).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::Path;

use wowdps_core::index::{self, SegmentMeta, load_segment_text};
use wowdps_core::meter::SegmentKind;
use wowdps_core::model::replay::{Cut, EventKind, PlacedKind, UnitKind};
use wowdps_core::replay::{PlacedTable, cut};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");
const GATED: [&str; 9] = [
    "sample", "taken", "support", "spans", "shields", "stacks", "tree", "relog", "replay",
];

/// The fixtures' placed spells — check.awk's `PLACE` / `TELL`, a subset of
/// the encounter rubric's generated table.
struct Fixtures;

impl PlacedTable for Fixtures {
    fn places(&self, spell: u32) -> bool {
        matches!(spell, 111_771 | 192_077 | 48_018)
    }
    fn tells(&self, spell: u32) -> bool {
        matches!(spell, 113_942 | 192_082 | 145_629 | 73_921)
    }
}

/// (cut — a pull's segment number, a run's "K<n>" —, player, floor or "*",
/// metric) → value, for the `replay_` rows.
type Golden = BTreeMap<(String, String, String), u64>;

fn golden(name: &str) -> Golden {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}.expected.tsv")).unwrap();
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let [seg, _, _, _, _, _, _, player, metric, value] = f[..] else {
                return None;
            };
            metric.starts_with("replay_").then(|| {
                (
                    (seg.to_string(), player.to_string(), metric.to_string()),
                    value.parse().unwrap(),
                )
            })
        })
        .collect()
}

/// A cut as check.awk counts it.
fn counts(seg: &str, c: &Cut, out: &mut Golden) {
    let mut put = |player: &str, metric: &str, v: u64| {
        out.insert(
            (
                seg.to_string(),
                player.to_string(),
                format!("replay_{metric}"),
            ),
            v,
        );
    };
    put("*", "units", c.units.len() as u64);
    put("*", "posts", c.posts() as u64);
    put("*", "floor", u64::from(c.floor));
    for k in EventKind::ALL {
        let n = c.events.iter().filter(|e| e.kind == k).count();
        put("*", k.word(), n as u64);
    }
    for k in PlacedKind::ALL {
        let n = c.placed.iter().filter(|p| p.kind == k).count();
        put("*", &format!("placed_{}", k.word()), n as u64);
    }
    put("*", "markers", c.markers.len() as u64);
    for m in c.maps() {
        put(&m.map_id.to_string(), "map_posts", m.posts as u64);
    }
    for u in c
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Player && !u.posts.is_empty())
    {
        put(&u.guid, "posts", u.posts.len() as u64);
    }
}

/// The keystone runs a log holds whole, in order: what the store cuts for
/// a key (a keyed visit's Σ, its par timers known).
fn keys(idx: &index::Index) -> Vec<&SegmentMeta> {
    idx.overalls
        .iter()
        .filter(|m| m.pars_ms.is_some() && m.end_ms.is_some())
        .collect()
}

#[test]
fn every_fixtures_cut_matches_its_golden() {
    for name in GATED {
        let path = format!("{FIXTURES}/{name}.txt");
        let mut file = std::fs::File::open(&path).unwrap();
        let idx = index::scan(&mut file);
        let mut actual = Golden::new();
        let mut cut_any = false;
        for (i, meta) in idx.segments.iter().enumerate() {
            if meta.kind != SegmentKind::Encounter || meta.arena || meta.encounter.is_none() {
                continue;
            }
            let text = load_segment_text(Path::new(&path), meta).unwrap();
            let c = cut(text.seeds(), text.slice(), &Fixtures, None);
            counts(&(i + 1).to_string(), &c, &mut actual);
            cut_any = true;
        }
        for (i, meta) in keys(&idx).into_iter().enumerate() {
            let text = load_segment_text(Path::new(&path), meta).unwrap();
            let c = cut(text.seeds(), text.slice(), &Fixtures, None);
            counts(&format!("K{}", i + 1), &c, &mut actual);
        }
        assert!(cut_any, "{name}: no boss pull to cut");
        let want = golden(name);
        assert!(!want.is_empty(), "{name}: the golden holds no replay rows");
        let mismatches: Vec<String> = want
            .keys()
            .chain(actual.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter(|k| want.get(*k) != actual.get(*k))
            .map(|k| format!("{k:?}: golden {:?}, cut {:?}", want.get(k), actual.get(k)))
            .collect();
        assert!(mismatches.is_empty(), "{name}:\n{}", mismatches.join("\n"));
    }
}

/// The index's seeds carry everything the cut reads from before its slice
/// (COMBATANT_INFO, ZONE_CHANGE, CHALLENGE_MODE, WORLD_MARKER), so a pull
/// or a keystone run cut lazily is the one cut with the whole log before
/// it.
#[test]
fn a_lazy_cut_is_the_full_cut() {
    for name in GATED {
        let path = format!("{FIXTURES}/{name}.txt");
        let bytes = std::fs::read(&path).unwrap();
        let mut file = std::fs::File::open(&path).unwrap();
        let idx = index::scan(&mut file);
        let pulls = idx
            .segments
            .iter()
            .filter(|m| m.kind == SegmentKind::Encounter && !m.arena);
        for meta in pulls.chain(keys(&idx)) {
            let text = load_segment_text(Path::new(&path), meta).unwrap();
            let lazy = cut(text.seeds(), text.slice(), &Fixtures, Some("Player-1-A"));
            let before = String::from_utf8_lossy(&bytes[..meta.byte_range.0 as usize]).into_owned();
            let full = cut(
                before.lines().map(|l| l.trim_end_matches('\r')),
                text.slice(),
                &Fixtures,
                Some("Player-1-A"),
            );
            assert_eq!(lazy, full, "{name}: {}", meta.name);
        }
    }
}

/// The replay fixture's kill, cut: the floor the players stood on most and
/// the hunter's steps onto another (his posts and the hit that found him
/// there kept), its boss rows, a feigning hunter alive, a creature going
/// down unconscious down, the markers standing (one replaced before the
/// pull), a warlock's class from R8, a sourceless hit naming no unit.
#[test]
fn the_replay_fixtures_kill_reads_as_its_ruling_says() {
    let path = format!("{FIXTURES}/replay.txt");
    let mut file = std::fs::File::open(&path).unwrap();
    let idx = index::scan(&mut file);
    let meta = &idx.segments[0];
    let text = load_segment_text(Path::new(&path), meta).unwrap();
    let c = cut(text.seeds(), text.slice(), &Fixtures, Some("Player-1-A"));
    assert_eq!(c.floor, 2434);
    let hunt = c.units.iter().find(|u| u.guid == "Player-1-H").unwrap();
    let floors: Vec<(u32, u32)> = hunt.posts.iter().map(|p| (p.t_ms, p.map_id)).collect();
    assert_eq!(floors, [(1500, 2434), (9800, 2435), (9900, 2435)]);
    let on_2435: Vec<Option<(i32, i32)>> = c
        .events
        .iter()
        .filter(|e| e.kind == EventKind::Hit && e.t_ms == 9900)
        .map(|e| e.at)
        .collect();
    assert_eq!(on_2435, [Some((1050, 2050))], "where his post there says");
    let bosses: Vec<(u32, EventKind, Option<u32>, u32, &str)> = c
        .events
        .iter()
        .filter(|e| e.kind.is_boss())
        .map(|e| (e.t_ms, e.kind, e.unit, e.spell_id, e.spell.as_str()))
        .collect();
    assert_eq!(
        bosses,
        [
            (0, EventKind::BossEngaged, None, 3000, "Ula'tek"),
            (30_000, EventKind::BossKilled, None, 3000, "Ula'tek"),
        ]
    );
    let you: Vec<&str> = c
        .units
        .iter()
        .filter(|u| u.you)
        .map(|u| u.name.as_str())
        .collect();
    assert_eq!(you, ["Tank-Realm-US"]);
    let lock = c.units.iter().find(|u| u.guid == "Player-1-W").unwrap();
    assert_eq!(
        (lock.class, lock.spec),
        (
            Some(wowdps_core::model::Class::Warlock),
            Some(wowdps_core::model::Spec::Destruction)
        ),
        "R8 from Incinerate"
    );
    let coil = c
        .units
        .iter()
        .find(|u| u.name == "Coil, the Lesser")
        .unwrap();
    assert_eq!((coil.kind, coil.npc), (UnitKind::Add, 502));
    let deaths: Vec<(EventKind, &str)> = c
        .events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Death | EventKind::NpcDied))
        .map(|e| (e.kind, c.units[e.unit.unwrap() as usize].name.as_str()))
        .collect();
    assert_eq!(
        deaths,
        [
            (EventKind::NpcDied, "Egg"),
            (EventKind::NpcDied, "Coil, the Lesser"),
            (EventKind::Death, "Tank-Realm-US"),
        ],
        "the hunter feigned; the coil went down unconscious"
    );
    let rocks = c
        .events
        .iter()
        .find(|e| e.spell == "Falling Rocks")
        .unwrap();
    assert_eq!((rocks.kind, rocks.src), (EventKind::Hit, None));
    // Spirit Link Totem's redistribution (its last lines' neutral 0xa28) is
    // one of ours by its summon: no hit, and the totem no unit.
    assert!(c.events.iter().all(|e| e.spell != "Spirit Link"));
    assert!(c.units.iter().all(|u| u.name != "Spirit Link Totem"));
    let markers: Vec<(u32, u8)> = c.markers.iter().map(|m| (m.t_ms, m.marker)).collect();
    assert_eq!(
        markers,
        [(0, 1), (0, 4), (6000, 4), (6250, 7), (6260, 5), (6350, 5)],
        "5 placed here, then on another map: it left"
    );
    assert_eq!(
        c.markers[5].kind,
        wowdps_core::model::replay::MarkerKind::Removed
    );
    assert_eq!(
        c.markers[0].at,
        Some((9500, -1200)),
        "the marker replaced before the pull"
    );
    assert_eq!(c.head.zone, "The Venomous Abyss");
    assert_eq!(
        (c.head.success, c.head.fight_ms),
        (Some(true), Some(30_000))
    );
}

/// The replay fixture's keystone run, cut whole: its trash on the upper
/// floor, its boss on the lower, the boss's rows mid-run where the
/// ENCOUNTER lines stand, the main floor the players' most posted.
#[test]
fn the_replay_fixtures_key_follows_the_party_down() {
    let path = format!("{FIXTURES}/replay.txt");
    let mut file = std::fs::File::open(&path).unwrap();
    let idx = index::scan(&mut file);
    let keys = keys(&idx);
    assert_eq!(keys.len(), 1, "one run");
    let text = load_segment_text(Path::new(&path), keys[0]).unwrap();
    let c = cut(text.seeds(), text.slice(), &Fixtures, Some("Player-1-A"));
    assert_eq!(c.head.encounter, None);
    assert_eq!(
        c.head.key.as_ref().map(|k| (k.name.as_str(), k.level)),
        Some(("The Glass Hollow", 14))
    );
    assert_eq!((c.head.map, c.head.success), (2521, Some(true)));
    let bosses: Vec<(u32, EventKind, u32, &str)> = c
        .events
        .iter()
        .filter(|e| e.kind.is_boss())
        .map(|e| (e.t_ms, e.kind, e.spell_id, e.spell.as_str()))
        .collect();
    assert_eq!(
        bosses,
        [
            (59_000, EventKind::BossEngaged, 3010, "Frost Warden"),
            (90_000, EventKind::BossKilled, 3010, "Frost Warden"),
        ]
    );
    let maps: Vec<(u32, usize, usize)> = c
        .maps()
        .iter()
        .map(|m| (m.map_id, m.posts, m.players))
        .collect();
    assert_eq!(maps, [(2094, 3, 2), (2095, 4, 3)]);
    assert_eq!(c.floor, 2095);
    let tank = c.units.iter().find(|u| u.you).unwrap();
    let floors: Vec<u32> = tank.posts.iter().map(|p| p.map_id).collect();
    assert_eq!(floors, [2094, 2094, 2095]);
    let kinds: Vec<(&str, UnitKind)> = c
        .units
        .iter()
        .filter(|u| u.kind != UnitKind::Player)
        .map(|u| (u.name.as_str(), u.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Glass Drake", UnitKind::Add),
            ("Frost Warden", UnitKind::Boss)
        ],
        "the boss rule names the run's boss by its encounter"
    );
}
