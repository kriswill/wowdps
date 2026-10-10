//! R29 (v45): the replay cut over every gated fixture, against the golden
//! TSVs `fixtures/check.awk` computes from the log grammar alone — the
//! floor, the posts (per player and in all), the units numbered, every
//! event kind, every placed kind and the world markers of each closed boss
//! pull — and the cut from a lazily loaded segment (its index seeds) equal
//! to the cut from every line before it (a full replay).

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::Path;

use wowdps_core::index::{self, load_segment_text};
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

/// (segment 1-based, player or "*", metric) → value, for the `replay_` rows.
type Golden = BTreeMap<(usize, String, String), u64>;

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
                    (seg.parse().unwrap(), player.to_string(), metric.to_string()),
                    value.parse().unwrap(),
                )
            })
        })
        .collect()
}

/// A cut as check.awk counts it.
fn counts(seg: usize, c: &Cut, out: &mut Golden) {
    let mut put = |player: &str, metric: &str, v: u64| {
        out.insert((seg, player.to_string(), format!("replay_{metric}")), v);
    };
    put("*", "units", c.units.len() as u64);
    put("*", "posts", c.posts() as u64);
    put("*", "floor", u64::from(c.floor));
    // The boss rows (format 2) are no kind the cut writes yet.
    for k in EventKind::ALL.into_iter().filter(|k| !k.is_boss()) {
        let n = c.events.iter().filter(|e| e.kind == k).count();
        put("*", k.word(), n as u64);
    }
    for k in PlacedKind::ALL {
        let n = c.placed.iter().filter(|p| p.kind == k).count();
        put("*", &format!("placed_{}", k.word()), n as u64);
    }
    put("*", "markers", c.markers.len() as u64);
    for u in c
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Player && !u.posts.is_empty())
    {
        put(&u.guid, "posts", u.posts.len() as u64);
    }
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
            counts(i + 1, &c, &mut actual);
            cut_any = true;
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
/// cut lazily is the pull cut with the whole log before it.
#[test]
fn a_lazy_cut_is_the_full_cut() {
    for name in GATED {
        let path = format!("{FIXTURES}/{name}.txt");
        let bytes = std::fs::read(&path).unwrap();
        let mut file = std::fs::File::open(&path).unwrap();
        let idx = index::scan(&mut file);
        for meta in idx
            .segments
            .iter()
            .filter(|m| m.kind == SegmentKind::Encounter && !m.arena)
        {
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

/// The replay fixture's kill, cut: the floor the players stood on most, a
/// feigning hunter alive, a creature going down unconscious down, the
/// markers standing (one replaced before the pull), a warlock's class from
/// R8, a sourceless hit naming no unit.
#[test]
fn the_replay_fixtures_kill_reads_as_its_ruling_says() {
    let path = format!("{FIXTURES}/replay.txt");
    let mut file = std::fs::File::open(&path).unwrap();
    let idx = index::scan(&mut file);
    let meta = &idx.segments[0];
    let text = load_segment_text(Path::new(&path), meta).unwrap();
    let c = cut(text.seeds(), text.slice(), &Fixtures, Some("Player-1-A"));
    assert_eq!(c.floor, 2434);
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
