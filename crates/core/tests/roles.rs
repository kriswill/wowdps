//! R30: roles inferred per fight for an edition whose log carries no spec.
//! The fixture (`fixtures/roles.txt`) is a hand-written Forever log of five
//! fights inside one dungeon visit: a boss with a warrior tank, a shaman
//! healer and three dealing damage (the first real Forever log's shape); a
//! boss with two tanks (a druid in Bear Form and a warrior), a priest
//! healer, a mage and a rogue who only dies; a six-second pull; trash with
//! no tank and no healer; and a solo shaman who heals only herself. The
//! goldens (`roles.expected.md` / `.tsv`, gated by `fixture_totals.rs` and
//! `check.awk`) name each role; these tests hold what the totals cannot:
//! that a lazy load reads the same roles, that the same lines under
//! Retail's header infer none, and that the visit's Overall reads its
//! members' ledgers merged.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::Path;

use wowdps_core::index::{load_segment_text, scan};
use wowdps_core::meter::{Meter, SegmentKind, meter_from_lines};
use wowdps_model::Role;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/roles.txt");

const WARRIOR: &str = "Player-6101-0C1D2E01";
const SHAMAN: &str = "Player-6101-0C1D2E02";
const HUNTER: &str = "Player-6101-0C1D2E03";
const PALADIN: &str = "Player-6101-0C1D2E04";
const SHAMAN_DPS: &str = "Player-6101-0C1D2E05";
const DRUID: &str = "Player-6101-0C1D2E06";
const PRIEST: &str = "Player-6101-0C1D2E07";
const MAGE: &str = "Player-6101-0C1D2E08";
/// Swung at once and dead: no damage, no healing, no cast.
const ROGUE: &str = "Player-6101-0C1D2E09";

fn text() -> String {
    std::fs::read_to_string(FIXTURE).unwrap()
}

fn roles(pairs: &[(&str, Role)]) -> BTreeMap<String, Role> {
    pairs.iter().map(|(g, r)| (g.to_string(), *r)).collect()
}

/// Each fight's roles, in the log's order.
fn every_fight(meter: &Meter) -> Vec<BTreeMap<String, Role>> {
    meter
        .segments()
        .iter()
        .map(|s| s.inferred_roles())
        .collect()
}

fn expected() -> Vec<BTreeMap<String, Role>> {
    use Role::{Dps, Healer, Tank};
    vec![
        roles(&[
            (WARRIOR, Tank),
            (SHAMAN, Healer),
            (HUNTER, Dps),
            (PALADIN, Dps),
            (SHAMAN_DPS, Dps),
        ]),
        // The rogue who only died is named nothing.
        roles(&[
            (DRUID, Tank),
            (WARRIOR, Tank),
            (PRIEST, Healer),
            (MAGE, Dps),
        ]),
        // Six seconds of combat: nobody is named.
        BTreeMap::new(),
        // No tank, no healer: whoever the enemy hit most, and whoever
        // healed another once, still deal damage.
        roles(&[(HUNTER, Dps), (MAGE, Dps), (SHAMAN_DPS, Dps)]),
        // Alone, healing only herself.
        roles(&[(SHAMAN, Dps)]),
    ]
}

#[test]
fn each_fight_names_the_roles_its_own_lines_show() {
    let meter = meter_from_lines(text().lines());
    let kinds: Vec<SegmentKind> = meter.segments().iter().map(|s| s.kind).collect();
    use SegmentKind::{Encounter, Trash};
    assert_eq!(kinds, [Encounter, Encounter, Trash, Trash, Trash]);
    assert_eq!(every_fight(&meter), expected());
    // The rogue is on the second fight's card all the same, with a score.
    let second = &meter.segments()[1];
    let rogue = second
        .role_scores()
        .into_iter()
        .find(|s| s.evidence.guid == ROGUE)
        .unwrap();
    assert_eq!((rogue.role, rogue.evidence.swings), (None, 1));
}

/// Each closed fight loaded lazily, alone, reads the scores the full replay
/// does: nothing a role needs is carried between fights.
#[test]
fn a_lazy_load_reads_the_same_roles() {
    let full = meter_from_lines(text().lines());
    let idx = scan(&mut std::fs::File::open(FIXTURE).unwrap());
    // The scan lists the closed fights: all but the solo one, still open
    // at the log's end.
    let closed = full
        .segments()
        .iter()
        .filter(|s| s.end_ms.is_some())
        .count();
    assert_eq!((idx.segments.len(), closed), (4, 4));
    for meta in &idx.segments {
        let seg = full
            .segments()
            .iter()
            .find(|s| s.start_ms == meta.start_ms)
            .unwrap();
        let lazy = load_segment_text(Path::new(FIXTURE), meta).unwrap().meter();
        let mine = lazy
            .segments()
            .iter()
            .find(|s| s.start_ms == seg.start_ms)
            .unwrap();
        assert_eq!(mine.role_scores(), seg.role_scores(), "{}", seg.name);
    }
}

/// Retail's role is its spec's: the same lines under Retail's header (or a
/// Classic client's, or an edition the build does not know) infer none.
#[test]
fn an_edition_with_no_signal_table_infers_nothing() {
    let text = text();
    assert!(text.contains(",PROJECT_ID,18\n"));
    for id in [0, 1, 2, 7] {
        let other = text.replace(",PROJECT_ID,18\n", &format!(",PROJECT_ID,{id}\n"));
        let meter = meter_from_lines(other.lines());
        for seg in meter.segments() {
            assert!(
                seg.role_scores().is_empty(),
                "PROJECT_ID {id}: {}",
                seg.name
            );
        }
    }
}

/// The visit's Overall weighs its members' ledgers merged, on their summed
/// clocks: over the whole run the warrior and the druid tank, the shaman
/// and the priest heal, the rest deal damage and the rogue is named nothing.
#[test]
fn the_overall_reads_its_members_merged() {
    use Role::{Dps, Healer, Tank};
    let meter = meter_from_lines(text().lines());
    assert_eq!(meter.visits().len(), 1);
    let overall = meter.overall(0).unwrap();
    assert_eq!(overall.kind, SegmentKind::Overall);
    assert_eq!(
        overall.inferred_roles(),
        roles(&[
            (WARRIOR, Tank),
            (DRUID, Tank),
            (SHAMAN, Healer),
            (PRIEST, Healer),
            (HUNTER, Dps),
            (PALADIN, Dps),
            (SHAMAN_DPS, Dps),
            (MAGE, Dps),
        ])
    );
    let warrior = overall
        .role_scores()
        .into_iter()
        .find(|s| s.evidence.guid == WARRIOR)
        .unwrap();
    // Swings 8 + 5 + 3, the kit 4 + 3 + 1, and the stance's 29 s of fight
    // one inside the run's 111 s.
    assert_eq!(
        (
            warrior.evidence.swings,
            warrior.evidence.kit,
            warrior.evidence.form_ms
        ),
        (16, 8, 29_000)
    );
}

/// The solver reads its fight alone: the same boss fight at the end of a
/// log of other fights, or alone in a log of its own, names the same roles.
#[test]
fn a_fight_reads_the_same_wherever_it_sits() {
    let text = text();
    let mut lines: Vec<&str> = text.lines().collect();
    // The header and the zone, then the second boss alone.
    let start = lines
        .iter()
        .position(|l| l.contains("ENCOUNTER_START,2910"))
        .unwrap();
    let end = lines
        .iter()
        .position(|l| l.contains("ENCOUNTER_END,2910"))
        .unwrap();
    let alone: Vec<&str> = lines[..2]
        .iter()
        .chain(&lines[start..=end])
        .copied()
        .collect();
    let meter = meter_from_lines(alone);
    assert_eq!(meter.segments()[0].inferred_roles(), expected()[1]);
    // And with every other fight's lines dropped from before it.
    lines.drain(2..start);
    let meter = meter_from_lines(lines);
    assert_eq!(meter.segments()[0].inferred_roles(), expected()[1]);
}
