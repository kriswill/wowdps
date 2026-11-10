//! R8 per edition: the Forever fixture (`fixtures/forever.txt`) read by
//! Forever's tables alone. Its header is a Forever client's (PROJECT_ID 18,
//! build 1.60.1, ADVANCED_LOG_ENABLED 0 over lines that still carry the
//! advanced block), it has no COMBATANT_INFO, and five players fight one
//! boss: each player's class is the one Forever's generated class table
//! gives, none gets a spec, none is read off Retail's table (the same lines
//! under Retail's header read otherwise), and a spell no table knows is
//! counted like any other. The lazy load, a resumed scan and the replay cut
//! read the same edition the full replay does. Goldens in
//! `forever.expected.md` / `.tsv`; the totals are gated by
//! `fixture_totals.rs`.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

use std::collections::BTreeMap;

use wowdps_core::index::{load_segment_text, scan, scan_from};
use wowdps_core::meter::{Meter, Segment, SegmentKind, View, meter_from_lines};
use wowdps_core::model::{Class, Spec};
use wowdps_core::replay::{NoPlaced, cut};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/forever.txt");

const ROGUE: &str = "Player-6101-0A2B3C01";
const MAGE: &str = "Player-6101-0A2B3C02";
const WARRIOR: &str = "Player-6101-0A2B3C03";
const PRIEST: &str = "Player-6101-0A2B3C04";
/// Casts only spells Forever's class table does not hold: Stormstrike (a
/// Shaman's in Retail's table) and Flametongue Attack (in no table).
const STRANGER: &str = "Player-6101-0A2B3C05";

fn text() -> String {
    std::fs::read_to_string(FIXTURE).unwrap()
}

/// The fixture with its header naming another `PROJECT_ID`.
fn as_project(id: u8) -> String {
    let text = text();
    assert!(text.contains(",PROJECT_ID,18\n"));
    text.replace(",PROJECT_ID,18\n", &format!(",PROJECT_ID,{id}\n"))
}

fn boss_pull(meter: &Meter) -> &Segment {
    let pulls: Vec<_> = meter
        .segments()
        .iter()
        .filter(|s| s.kind == SegmentKind::Encounter)
        .collect();
    assert_eq!(pulls.len(), 1);
    pulls[0]
}

/// Every player on the Damage view by guid: (class, spec).
fn classes(seg: &Segment) -> BTreeMap<String, (Option<Class>, Option<Spec>)> {
    seg.rows(View::Damage)
        .into_iter()
        .map(|r| (r.key, (r.class, r.spec)))
        .collect()
}

/// Every view's rows as (view, guid, amount, extra).
fn amounts(seg: &Segment) -> Vec<(String, String, u64, u64)> {
    [View::Damage, View::Healing, View::Taken]
        .into_iter()
        .flat_map(|v| {
            seg.rows(v)
                .into_iter()
                .map(move |r| (format!("{v:?}"), r.key, r.amount, r.extra))
        })
        .collect()
}

fn forever_reading() -> BTreeMap<String, (Option<Class>, Option<Spec>)> {
    [
        (ROGUE, Some(Class::Rogue)),
        (MAGE, Some(Class::Mage)),
        (WARRIOR, Some(Class::Warrior)),
        (PRIEST, Some(Class::Priest)),
        (STRANGER, None),
    ]
    .into_iter()
    .map(|(g, c)| (g.to_string(), (c, None)))
    .collect()
}

/// Each player's class is Forever's table's, and nobody has a spec: Expose
/// Armor makes the Rogue a Rogue, Fireball a Mage, Defensive Stance a
/// Warrior, the shield and the heal a Priest; the fifth player's spells are
/// in no Forever table, so he has no class at all.
#[test]
fn each_player_gets_forevers_class_and_no_spec() {
    let meter = meter_from_lines(text().lines());
    let seg = boss_pull(&meter);
    assert_eq!(seg.project_id, 18);
    assert_eq!(seg.build, (1, 60, 1));
    assert_eq!(classes(seg), forever_reading());
}

/// Nothing is read from Retail's table: the same lines under Retail's
/// header read as Retail's table says — the id Forever gives a Rogue is a
/// Monk's there, Fireball names Fire, Defensive Stance names Protection,
/// Stormstrike a Shaman — and under a Classic client's, which has no table,
/// nobody has a class.
#[test]
fn nothing_is_read_from_retails_table() {
    let retail = meter_from_lines(as_project(1).lines());
    let want: BTreeMap<String, (Option<Class>, Option<Spec>)> = [
        (ROGUE, Some(Class::Monk), None),
        (MAGE, Some(Class::Mage), Spec::from_id(63)),
        (WARRIOR, Some(Class::Warrior), Spec::from_id(73)),
        (PRIEST, Some(Class::Priest), None),
        (STRANGER, Some(Class::Shaman), None),
    ]
    .into_iter()
    .map(|(g, c, s)| (g.to_string(), (c, s)))
    .collect();
    assert_eq!(classes(boss_pull(&retail)), want);
    assert_ne!(want, forever_reading());

    for id in [2, 5, 11, 14, 19, 7] {
        let other = meter_from_lines(as_project(id).lines());
        let read = classes(boss_pull(&other));
        assert!(
            read.values().all(|&(c, s)| c.is_none() && s.is_none()),
            "PROJECT_ID {id}: {read:?}"
        );
    }
}

/// Every edition counts the same amounts: the tables only name classes,
/// shields and roles, so damage, healing (absorbs included) and damage
/// taken are the log's under every header.
#[test]
fn every_edition_counts_the_same_amounts() {
    let forever = meter_from_lines(text().lines());
    let want = amounts(boss_pull(&forever));
    assert!(!want.is_empty());
    for id in [0, 1, 2, 7] {
        let other = meter_from_lines(as_project(id).lines());
        assert_eq!(amounts(boss_pull(&other)), want, "PROJECT_ID {id}");
    }
}

/// A spell no table knows breaks nothing: Flametongue Attack's 45 is on
/// the fifth player's Damage row and its own by-ability row, beside the
/// Stormstrike Forever's class table does not hold either.
#[test]
fn an_id_no_table_knows_is_counted() {
    let meter = meter_from_lines(text().lines());
    let seg = boss_pull(&meter);
    let row = seg
        .rows(View::Damage)
        .into_iter()
        .find(|r| r.key == STRANGER)
        .unwrap();
    assert_eq!((row.amount, row.class, row.spec), (205, None, None));
    let (spells, _) = seg.breakdown(STRANGER, View::Damage);
    let by: BTreeMap<String, (u32, u64)> = spells
        .into_iter()
        .map(|r| (r.label, (r.spell_id, r.amount)))
        .collect();
    assert_eq!(by.get("Flametongue Attack"), Some(&(29470, 45)));
    assert_eq!(by.get("Stormstrike"), Some(&(17364, 160)));
    // The tree has no item or proc table to hang either under.
    let tree = seg.spell_tree(STRANGER, View::Damage);
    assert!(tree.groups.is_empty(), "{:?}", tree.groups);
}

/// The tables Forever has none of add nothing, and take nothing away: the
/// Warrior's Shield Wall (a defensive in Retail's role table) opens no
/// span, and the Priest's Power Word: Shield (Retail's absorb table) opens
/// no shield, yet the 200 it absorbed is her healing and on her shield
/// ledger, whose size it never learned.
#[test]
fn a_table_forever_lacks_adds_nothing_and_takes_nothing() {
    let meter = meter_from_lines(text().lines());
    let seg = boss_pull(&meter);
    assert!(seg.spans(WARRIOR).is_empty());
    assert_eq!(seg.am_uptime_ms(WARRIOR), 0);
    assert_eq!(seg.absorbed_healing(PRIEST), 200);
    assert_eq!(seg.absorb_wasted(PRIEST), None);
    assert_eq!(seg.shields_unknown(PRIEST), 1);
    let shields = seg.shields(PRIEST);
    assert_eq!(shields.len(), 1);
    assert_eq!((shields[0].spell_id, shields[0].consumed), (17, 200));

    // Under Retail's header the same lines open both.
    let retail = meter_from_lines(as_project(1).lines());
    let seg = boss_pull(&retail);
    assert_eq!(seg.spans(WARRIOR).len(), 1);
    assert_eq!(seg.absorb_wasted(PRIEST), Some(50));
}

/// The pull loaded lazily — its seeds (the version line among them) then
/// its slice — reads Forever's tables as the full replay does.
#[test]
fn a_lazy_load_reads_the_same_edition() {
    let full = meter_from_lines(text().lines());
    let idx = scan(&mut std::fs::File::open(FIXTURE).unwrap());
    let meta = idx
        .segments
        .iter()
        .find(|m| m.kind == SegmentKind::Encounter)
        .unwrap();
    let lazy = load_segment_text(std::path::Path::new(FIXTURE), meta)
        .unwrap()
        .meter();
    let (a, b) = (boss_pull(&full), boss_pull(&lazy));
    assert_eq!(b.project_id, 18);
    assert_eq!(classes(b), classes(a));
    assert_eq!(amounts(b), amounts(a));
}

/// A scan resumed from any line's checkpoint keeps the edition the version
/// line named, and so reproduces the full scan.
#[test]
fn a_resumed_scan_reads_the_same_edition() {
    let text = text();
    let bytes = text.as_bytes();
    let full = scan(&mut &bytes[..]);
    assert_eq!(full.checkpoint.project_id, 18);
    for (i, _) in bytes.iter().enumerate().filter(|&(_, &b)| b == b'\n') {
        let prefix = scan(&mut &bytes[..=i]);
        let state = prefix.checkpoint.clone();
        let off = state.offset as usize;
        let resumed = scan_from(&mut &bytes[off..], state);
        assert_eq!(resumed, full, "cut after byte {i}");
    }
}

/// The replay cut reads the edition too: the Rogue, the one player it
/// numbers (his cast), is a Rogue with no spec, never Retail's Monk.
#[test]
fn the_cut_reads_forevers_classes() {
    let idx = scan(&mut std::fs::File::open(FIXTURE).unwrap());
    let meta = idx
        .segments
        .iter()
        .find(|m| m.kind == SegmentKind::Encounter)
        .unwrap();
    let text = load_segment_text(std::path::Path::new(FIXTURE), meta).unwrap();
    let c = cut(text.seeds(), text.slice(), &NoPlaced, None);
    let rogue = c.units.iter().find(|u| u.guid == ROGUE).unwrap();
    assert_eq!((rogue.class, rogue.spec), (Some(Class::Rogue), None));
}

/// A door onto a map Forever's own map table calls open world (2980,
/// Dalaran City) is zoned out whatever difficulty the game stamped on it,
/// in the meter and the scanner alike; Retail's table does not list that
/// map, so under Retail's header the same door opens a visit, and so it
/// does under a Classic client's, which reads Retail's map table.
#[test]
fn a_door_is_read_by_its_own_editions_map_table() {
    let log = |project: u8| {
        format!(
            "11/1/2026 19:00:00.000-8  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,{project}\n\
             11/1/2026 19:00:01.000-8  ZONE_CHANGE,48,\"Blackfathom Deeps\",1\n\
             11/1/2026 19:30:00.000-8  ZONE_CHANGE,2980,\"Dalaran City\",1\n"
        )
    };
    for (project, visits) in [(18, 1), (1, 2), (0, 2), (2, 2)] {
        let text = log(project);
        let meter = meter_from_lines(text.lines());
        assert_eq!(meter.visits().len(), visits, "PROJECT_ID {project}");
        let idx = scan(&mut text.as_bytes());
        assert_eq!(
            idx.checkpoint.visit_count, visits as u32,
            "PROJECT_ID {project}"
        );
    }
}

/// R10's hearth out of a raid: the game stamps the door onto the continent
/// with the raid's difficulty, and the continent's map is open world, so
/// the door zones out and the raid's visit stays the only one. Retail reads
/// its own map table, Forever its own (its dungeon, its Kalimdor), and an
/// edition with no map table of its own (a Classic client's) reads
/// Retail's, as before the tables were per edition: one visit in each, in
/// the meter and the scanner alike.
#[test]
fn a_hearth_out_of_a_raid_zones_out_in_every_edition() {
    let log = |project: u8, map: u32, zone: &str| {
        format!(
            "11/1/2026 19:00:00.000-8  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,1.0.0,PROJECT_ID,{project}\n\
             11/1/2026 19:00:01.000-8  ZONE_CHANGE,{map},\"{zone}\",9\n\
             11/1/2026 19:40:00.000-8  ZONE_CHANGE,1,\"Kalimdor\",9\n"
        )
    };
    for (project, map, zone) in [
        (1, 409, "Molten Core"),
        (2, 409, "Molten Core"),
        (18, 48, "Blackfathom Deeps"),
    ] {
        let text = log(project, map, zone);
        let meter = meter_from_lines(text.lines());
        assert_eq!(meter.visits().len(), 1, "PROJECT_ID {project}");
        let idx = scan(&mut text.as_bytes());
        assert_eq!(idx.checkpoint.visit_count, 1, "PROJECT_ID {project}");
    }
}

/// A Retail raid visit of one boss pull, whose trinket proc (Eradicating
/// Arcanocore, Retail's item table) groups under its item, then `tail`.
fn visit_log(tail: &str) -> String {
    format!(
        "10/1/2026 20:00:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.1.0,PROJECT_ID,1\n\
         10/1/2026 20:00:01.000-7  ZONE_CHANGE,2769,\"Liberation of Undermine\",16\n\
         10/1/2026 20:01:00.000-7  ENCOUNTER_START,3009,\"Vexie and the Geargrinders\",16,20,2769\n\
         10/1/2026 20:01:05.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Vexie\",0xa48,0x0,1233384,\"Eradicating Arcanocore\",64,500,500,0,0,0,0,0,nil,nil\n\
         10/1/2026 20:01:30.000-7  ENCOUNTER_END,3009,\"Vexie and the Geargrinders\",16,20,1,30000\n\
         {tail}"
    )
}

/// What an Overall is read by and what its tree says: (build, project id,
/// log version, the player's groups, the Damage rows).
type Read = ((u16, u16, u16), u8, u32, Vec<String>, Vec<(String, u64)>);

fn read_of(seg: &Segment) -> Read {
    let groups = seg
        .spell_tree("Player-1-A", View::Damage)
        .groups
        .into_iter()
        .map(|g| g.key)
        .collect();
    let rows = seg
        .rows(View::Damage)
        .into_iter()
        .map(|r| (r.key, r.amount))
        .collect();
    (seg.build, seg.project_id, seg.log_version, groups, rows)
}

/// The Σ of a visit is read by its members' game, never by a version line
/// after them: a full replay stands past the visit, where another edition's
/// version line has moved the meter, while a lazy load of the visit stands
/// at its end. Both read Retail's tables, as the pull itself does — the
/// trinket proc under its item — whether the visit closed before the line
/// (a door into another instance) or is still current when it comes.
#[test]
fn an_overall_reads_its_members_edition_lazy_or_full() {
    let forever = "10/1/2026 20:10:00.000-7  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18\n";
    let closed = visit_log(&format!(
        "10/1/2026 20:05:00.000-7  ZONE_CHANGE,2526,\"Algeth'ar Academy\",23\n{forever}"
    ));
    let current = visit_log(forever);
    for (name, text) in [("closed", &closed), ("current", &current)] {
        let full = meter_from_lines(text.lines());
        let pull = read_of(boss_pull(&full));
        assert_eq!(pull.1, 1, "{name}");
        assert_eq!(pull.3, ["item:242394"], "{name}");
        let want = read_of(&full.overall(0).unwrap());
        assert_eq!(want, pull, "{name}: the full replay's Σ");

        let path = std::env::temp_dir().join(format!(
            "wowdps-forever-overall-{name}-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, text).unwrap();
        let idx = scan(&mut std::fs::File::open(&path).unwrap());
        let meta = match name {
            "closed" => idx.overalls.iter().find(|m| m.visit == Some(0)).unwrap(),
            _ => idx.open_visit.as_ref().unwrap(),
        };
        let lazy = load_segment_text(&path, meta).unwrap().meter();
        assert_eq!(read_of(&lazy.overall(0).unwrap()), want, "{name}: lazy");
        std::fs::remove_file(&path).unwrap();
    }
}

/// The daemon's live Overall, attached mid-visit across an edition: the
/// visit's first pull is Retail's, a version line names another edition
/// and the visit resumes with a pull open. The live meter's own Σ is that
/// later pull's, the lazily loaded prefix holds the first; merged with
/// `Segment::absorb_prefix` the Σ reads as the full replay's — the first
/// member's build, edition and log version, Retail's item under the proc.
#[test]
fn a_live_overall_merged_with_its_prefix_reads_as_the_full_replay() {
    let attach = visit_log(
        "10/1/2026 20:02:00.000-7  COMBAT_LOG_VERSION,21,ADVANCED_LOG_ENABLED,0,BUILD_VERSION,1.60.1,PROJECT_ID,18\n\
         10/1/2026 20:02:01.000-7  ZONE_CHANGE,2769,\"Liberation of Undermine\",16\n\
         10/1/2026 20:03:00.000-7  ENCOUNTER_START,3010,\"Cauldron of Carnage\",16,20,2769\n\
         10/1/2026 20:03:05.000-7  SPELL_DAMAGE,Player-1-A,\"Ana-Realm\",0x511,0x0,Creature-0-1,\"Vexie\",0xa48,0x0,1233384,\"Eradicating Arcanocore\",64,300,300,0,0,0,0,0,nil,nil\n",
    );
    let whole = format!(
        "{attach}10/1/2026 20:03:30.000-7  ENCOUNTER_END,3010,\"Cauldron of Carnage\",16,20,1,30000\n"
    );
    let full = meter_from_lines(whole.lines());
    let want = read_of(&full.overall(0).unwrap());
    assert_eq!((want.0, want.1, want.2), ((12, 1, 0), 1, 22));

    let path =
        std::env::temp_dir().join(format!("wowdps-forever-attach-{}.txt", std::process::id()));
    std::fs::write(&path, &attach).unwrap();
    let idx = scan(&mut std::fs::File::open(&path).unwrap());
    let prefix = load_segment_text(&path, idx.open_visit.as_ref().unwrap())
        .unwrap()
        .meter();
    // The live side the way the tailer builds it: the open pull's seeds,
    // then everything from the live offset.
    let bytes = attach.as_bytes();
    let open = idx.open.as_ref().unwrap();
    let seeds: Vec<String> = open
        .seeds
        .iter()
        .map(|&(s, e)| {
            String::from_utf8_lossy(&bytes[s as usize..e as usize])
                .trim_end()
                .to_string()
        })
        .collect();
    let tail = String::from_utf8_lossy(&bytes[idx.live_offset as usize..]).into_owned();
    let mut lines: Vec<&str> = tail.lines().collect();
    let end = "10/1/2026 20:03:30.000-7  ENCOUNTER_END,3010,\"Cauldron of Carnage\",16,20,1,30000";
    lines.push(end);
    let live = wowdps_core::meter::meter_from_seeded(seeds.iter().map(String::as_str), lines);
    let mut combined = live.overall(0).unwrap();
    assert_eq!(
        combined.project_id, 18,
        "the live half alone reads the later edition"
    );
    combined.absorb_prefix(&prefix.overall(0).unwrap());
    let got = read_of(&combined);
    assert_eq!(
        (got.0, got.1, got.2, &got.3),
        (want.0, want.1, want.2, &want.3)
    );
    let total = |r: &Read| r.4.iter().map(|(_, a)| a).sum::<u64>();
    assert_eq!(total(&got), total(&want));
    std::fs::remove_file(&path).unwrap();
}
