//! R17 through the daemon: a Taken watch answers rate rows and — when
//! drilled — a `Breakdown` carrying the player's mitigation record (v21);
//! the history store's rows tier carries the Taken rows (its seventh view),
//! and since v22 the card's tank measures plus a per-player mitigation
//! record with both Taken drills, so `stored_fight(Taken)` equals the live
//! meter on every tier. Also here: the by-ability cap and its rollup, the
//! regrade back-fill of a pre-2b record, `Trend` by dtps / mitigated_pct,
//! the `Fights { role }` subject filter, and the protected set's tank
//! measure and its zero / aborted floor. Numbers are
//! `crates/core/fixtures/taken.expected.md`'s.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::path::Path;

use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{Backend, ClosedFight, LogFacts, MemBackend, Retention, Store};
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{MissKind, Role, Row, SegmentKind, Spec, View};
use wowdps_proto::history::{CardPlayer, FightCard, VIEW_KEYS};
use wowdps_proto::{
    Breakdown, ClientMsg, Cursor, DaemonMsg, FightSort, HistoryAnswer, HistoryQuery, SegmentRef,
    TrendBucket, TrendMeasure, TrendPoint,
};

const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/sample.txt");
const TAKEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/taken.txt");

const DURGAN: &str = "Player-1168-0A1B2C11";
const ZENLI: &str = "Player-1168-0A1B2C12";
const PYRALIS: &str = "Player-1168-0A1B2C13";

/// The segment ids the mock lists, in list order.
fn ids(mock: &mut MockDaemon) -> Vec<wowdps_model::SegmentId> {
    let out = mock.handle(ClientMsg::Watch(Cursor::List));
    let list = out
        .iter()
        .find_map(|m| match m {
            DaemonMsg::SegmentList { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .expect("a list answers the list cursor");
    list.iter().map(|e| e.id).collect()
}

/// Watch `view` on `id` (optionally drilled) and return the settled
/// snapshot's rows and breakdown.
fn watch(
    mock: &mut MockDaemon,
    id: wowdps_model::SegmentId,
    view: View,
    drill: Option<&str>,
) -> (wowdps_model::SegmentInfo, Vec<Row>, Option<Breakdown>) {
    let out = mock.handle(ClientMsg::Watch(Cursor::Segment {
        segment: SegmentRef::Id(id),
        view,
        top_n: None,
        drill: drill.map(str::to_string),
        spell: None,
    }));
    out.into_iter()
        .rev()
        .find_map(|m| match m {
            DaemonMsg::Snapshot {
                view: v,
                info,
                rows,
                breakdown,
                ..
            } if v == view => Some((info, rows, breakdown)),
            _ => None,
        })
        .expect("a snapshot answers the segment cursor")
}

/// The fixture's boss encounter, as the mock lists it.
fn boss(mock: &mut MockDaemon) -> wowdps_model::SegmentId {
    ids(mock)
        .into_iter()
        .find(|id| {
            let (info, _, _) = watch(mock, *id, View::Damage, None);
            info.kind == SegmentKind::Encounter && info.name == "Taken Test Boss"
        })
        .expect("taken.txt lists its boss")
}

fn row<'a>(rows: &'a [Row], key: &str) -> &'a Row {
    rows.iter()
        .find(|r| r.key == key)
        .unwrap_or_else(|| panic!("{key} has a row: {rows:?}"))
}

#[test]
fn a_taken_watch_answers_rate_rows_and_a_drill_carries_the_mitigation_record() {
    let mut mock = MockDaemon::fixture_at(Path::new(TAKEN));
    let boss = boss(&mut mock);

    // Undrilled: three rows (pets folded), DTPS over the 60 s kill, `extra`
    // = absorbed, no breakdown.
    let (info, rows, breakdown) = watch(&mut mock, boss, View::Taken, None);
    assert_eq!(info.duration_ms, 60_000);
    assert!(breakdown.is_none(), "no drill, no breakdown");
    assert_eq!(rows.len(), 3, "pets fold onto owners: {rows:?}");
    let durgan = row(&rows, DURGAN);
    assert_eq!((durgan.amount, durgan.extra), (84_000, 12_000));
    assert!(
        (durgan.per_sec - 1400.0).abs() < 1e-9,
        "DTPS: {}",
        durgan.per_sec
    );
    let zenli = row(&rows, ZENLI);
    assert_eq!((zenli.amount, zenli.extra), (70_200, 25_000));
    assert!(
        (zenli.per_sec - 1170.0).abs() < 1e-9,
        "DTPS: {}",
        zenli.per_sec
    );
    let pyralis = row(&rows, PYRALIS);
    assert_eq!((pyralis.amount, pyralis.extra), (52_000, 5_000));
    assert!(rows.iter().all(|r| r.per_sec > 0.0), "Taken is a rate view");
    assert_eq!(rows[0].key, DURGAN, "sorted by amount taken");

    // Drilled on the tank: by-ability / by-attacker rows, the taken series
    // (R18, v24), and the mitigation record.
    let (_, drilled, breakdown) = watch(&mut mock, boss, View::Taken, Some(DURGAN));
    assert_eq!(drilled, rows, "the drill leaves the meter rows alone");
    let b = breakdown.expect("a drilled Taken watch answers a breakdown");
    // R18 (v24): the Taken drill carries the taken series with the tank's
    // spans — `Segment::taken_timeline`.
    assert!(
        b.timeline.is_some(),
        "the taken timeline rides the drill (v24)"
    );
    assert!(b.spell_timeline.is_none());
    assert!(!b.by_spell.is_empty());
    assert!(!b.by_target.is_empty());
    let boss_row = row(&b.by_target, "Taken Test Boss");
    assert_eq!(boss_row.amount, 84_000, "attackers keyed by name");
    let m = b
        .mitigation
        .expect("Taken drill carries the mitigation record");
    assert_eq!(
        (m.absorbed, m.blocked, m.absorbed_full, m.blocked_full),
        (12_000, 18_000, 0, 55_000)
    );
    assert_eq!((m.stagger, m.stagger_ticked), (0, 0));
    assert_eq!(m.misses(), 5);
    for kind in [MissKind::Block, MissKind::Parry, MissKind::Dodge] {
        assert_eq!(m.misses[kind.index()], 1, "{kind:?}");
    }
    assert_eq!(m.misses[MissKind::Miss.index()], 2);
    assert_eq!(m.mitigated(), 85_000);

    // The monk: stagger reported, never added; the ticks excluded.
    let (_, _, breakdown) = watch(&mut mock, boss, View::Taken, Some(ZENLI));
    let m = breakdown
        .and_then(|b| b.mitigation)
        .expect("Zenlí's record");
    assert_eq!(
        (m.absorbed, m.stagger, m.stagger_ticked),
        (25_000, 25_000, 10_000)
    );
    assert_eq!((m.absorbed_full, m.blocked, m.blocked_full), (3_000, 0, 0));
    assert_eq!(m.misses(), 1);
    assert_eq!(m.mitigated(), 28_000);

    // The mage and the pet: the pre-summon hit folds, "Environment" is an
    // attacker, and the add's EVADE of the mage's own cast is nobody's miss.
    let (_, _, breakdown) = watch(&mut mock, boss, View::Taken, Some(PYRALIS));
    let b = breakdown.expect("Pyralis' breakdown");
    assert!(
        b.by_target.iter().any(|r| r.key == "Environment"),
        "{:?}",
        b.by_target
    );
    let m = b.mitigation.expect("Pyralis' record");
    assert_eq!((m.absorbed, m.absorbed_full), (5_000, 21_000));
    assert_eq!(m.misses(), 5);
    assert_eq!(
        m.misses[MissKind::Evade.index()],
        0,
        "the add evaded, not F"
    );
    for kind in [
        MissKind::Immune,
        MissKind::Absorb,
        MissKind::Deflect,
        MissKind::Reflect,
        MissKind::Resist,
    ] {
        assert_eq!(m.misses[kind.index()], 1, "{kind:?}");
    }

    // Any other drilled view keeps `mitigation` absent — present iff Taken.
    for view in [View::Damage, View::Healing, View::Deaths, View::Interrupts] {
        let (_, _, breakdown) = watch(&mut mock, boss, view, Some(DURGAN));
        let b = breakdown.expect("a drill answers a breakdown");
        assert!(b.mitigation.is_none(), "{view:?} carries no mitigation");
    }
    let (_, _, breakdown) = watch(&mut mock, boss, View::Damage, Some(DURGAN));
    assert!(
        breakdown.unwrap().timeline.is_some(),
        "Damage keeps its curve"
    );
}

#[test]
fn an_unknown_drill_under_taken_has_no_record() {
    let mut mock = MockDaemon::fixture_at(Path::new(TAKEN));
    let boss = boss(&mut mock);
    let (_, _, breakdown) = watch(&mut mock, boss, View::Taken, Some("Player-0-nobody"));
    let b = breakdown.expect("a drill always answers a breakdown");
    assert!(b.by_spell.is_empty() && b.by_target.is_empty());
    assert!(b.mitigation.is_none(), "nobody took nothing");
}

/// Replay a whole log through an engine the way the tail thread would,
/// collecting every `Closed` fight.
fn closed_fights(path: &Path) -> Vec<ClosedFight> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.to_path_buf()), &mut events);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    engine.on_tail(TailEvent::Lines(lines), &mut events);
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::Closed(id) => engine.take_closed(*id),
            EngineEvent::Opened(_) => None,
        })
        .collect()
}

#[test]
fn the_rows_tier_carries_taken_as_its_seventh_view_for_every_fight() {
    assert_eq!(VIEW_KEYS[View::Taken.index()], (View::Taken, "taken"));
    assert_eq!(View::Taken.index(), 6);

    for log in [SAMPLE, TAKEN] {
        let path = Path::new(log);
        let facts = LogFacts::read(path);
        let mut store = Store::open(MemBackend::new(), Retention::default());
        let fights = closed_fights(path);
        assert!(!fights.is_empty(), "{log} closes fights");
        let mut any_taken = false;
        let mut stored = 0;
        for fight in &fights {
            // The store declines what it never keeps (out-of-instance
            // trash); every kept fight carries the seventh view.
            let Some(id) = store.store(fight, facts) else {
                continue;
            };
            stored += 1;
            let live = fight.segment.rows(View::Taken);
            any_taken |= !live.is_empty();

            // Read back through the JSON codec: the seventh slot IS the
            // live meter's Taken rows.
            let rows = store.rows(&id).expect("rows tier");
            assert_eq!(rows.views[6], live, "{log} {id}: views[6]");
            assert_eq!(rows.rows(View::Taken), &live[..]);
            let json = rows.to_json().to_line();
            assert!(json.contains("\"taken\":["), "{json}");

            // `stored_fight(Taken)` serves those rows AND (step 2b) the
            // drill, from the rows tier: both lists and the record.
            let drill = live.first().map(|r| r.key.clone());
            let sf = store
                .stored_fight(&id, View::Taken, drill.as_deref())
                .expect("the card exists");
            assert_eq!(sf.rows, live);
            assert!(sf.tier >= 2, "answered from the rows tier: {}", sf.tier);
            if let Some(guid) = &drill {
                let (by_spell, by_target) = fight.segment.breakdown(guid, View::Taken);
                let b = sf.breakdown.expect("the stored Taken drill");
                assert_eq!(b.by_target, by_target, "{log} {id}: by-attacker rows");
                assert_eq!(
                    b.by_spell, by_spell,
                    "{log} {id}: no fixture player has 16+ taken abilities, so the \
                     capped list is the meter's own, order included"
                );
                assert_eq!(b.mitigation, fight.segment.mitigation(guid));
                assert!(b.timeline.is_none(), "no taken curve until step 4");
            }
        }
        assert!(stored >= 1, "{log}: {stored} fights stored");
        assert!(any_taken, "{log} has friendly-destination damage");
    }
}

#[test]
fn the_stored_taken_rows_equal_the_live_snapshot_through_the_mock() {
    let mut mock = MockDaemon::fixture_at(Path::new(TAKEN)).with_history();
    let boss = boss(&mut mock);
    let (_, live, live_drill) = watch(&mut mock, boss, View::Taken, Some(DURGAN));
    let live_drill = live_drill.expect("the live Taken drill");

    let cards: Vec<_> = mock
        .history()
        .cards()
        .iter()
        .filter(|c| c.name == "Taken Test Boss")
        .cloned()
        .collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    let out = mock.handle(ClientMsg::GetFight {
        req_id: 7,
        fight_id: cards[0].id.clone(),
        view: View::Taken,
        drill: Some(DURGAN.to_string()),
        boss: None,
    });
    let [
        DaemonMsg::Fight {
            req_id: 7,
            fight: Some(f),
        },
    ] = out.as_slice()
    else {
        panic!("{out:?}");
    };
    assert_eq!(f.rows, live, "stored Taken rows are the live rows");
    let b = f.breakdown.clone().expect("the stored Taken drill");
    // R18 (v24): the live drill carries the taken timeline; the stored one
    // answers from the rows tier, which holds no taken series until the
    // coarse timeline lands there (spec §4.5, step 4b) — everything else
    // is identical.
    assert!(live_drill.timeline.is_some());
    assert!(b.timeline.is_none(), "no taken series on the rows tier yet");
    let live_drill = Breakdown {
        timeline: None,
        ..live_drill
    };
    assert_eq!(b, live_drill, "the stored drill IS the live drill");
}

// ---- step 2b: the store's tank measures ------------------------------------------

/// A temp directory of synthetic logs, removed on drop.
struct Temp(std::path::PathBuf);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("wowdps-taken-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
    /// Write a log and replay it: (path, its facts, its closed fights).
    fn log(&self, name: &str, text: &str) -> (std::path::PathBuf, LogFacts, Vec<ClosedFight>) {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        let facts = LogFacts::read(&path);
        let fights = closed_fights(&path);
        (path, facts, fights)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The fixture through the store: (store, its fights, facts, the kill's id).
fn taken_store(cfg: Retention) -> (Store<MemBackend>, Vec<ClosedFight>, LogFacts, String) {
    let path = Path::new(TAKEN);
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    let mut store = Store::open(MemBackend::new(), cfg);
    let mut kill = None;
    for f in &fights {
        if let Some(id) = store.store(f, facts)
            && f.segment.name == "Taken Test Boss"
        {
            kill = Some(id);
        }
    }
    let kill = kill.expect("the boss pull is stored");
    (store, fights, facts, kill)
}

fn player<'a>(card: &'a wowdps_proto::history::FightCard, guid: &str) -> &'a CardPlayer {
    card.players
        .iter()
        .find(|p| p.guid == guid)
        .unwrap_or_else(|| panic!("{guid} on the card"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn the_card_carries_the_tank_measures_and_the_rows_tier_the_mitigation_lists() {
    let (store, fights, _, kill) = taken_store(Retention::default());
    let card = store.card(&kill).expect("the kill's card");
    assert_eq!(card.duration_ms, 60_000);

    // taken.expected.md, segment 1: taken (amount + absorbed, stagger
    // self-ticks excluded), mitigated = partial absorbs + partial blocks +
    // full absorbs + full blocks, prevented = the full-miss amounts, dtps
    // over the 60 s kill.
    for (guid, taken, mitigated, prevented) in [
        (DURGAN, 84_000u64, 85_000u64, 55_000u64),
        (ZENLI, 70_200, 28_000, 3_000),
        (PYRALIS, 52_000, 26_000, 21_000),
    ] {
        let p = player(card, guid);
        assert_eq!(
            (p.taken, p.mitigated, p.prevented),
            (taken, mitigated, prevented),
            "{guid}"
        );
        assert!(
            close(p.dtps, taken as f64 / 60.0),
            "{guid} dtps {} over 60 s of {taken}",
            p.dtps
        );
        assert!(
            close(
                p.mitigated_pct(),
                mitigated as f64 * 100.0 / (taken + prevented) as f64
            ),
            "{guid} pct {}",
            p.mitigated_pct()
        );
    }
    // The exact numbers the fixture derives, so a rounding change is loud.
    assert!(close(player(card, DURGAN).dtps, 1400.0));
    assert!(close(player(card, ZENLI).dtps, 1170.0));
    assert!(close(
        player(card, DURGAN).mitigated_pct(),
        85_000.0 * 100.0 / 139_000.0
    ));

    // The derived pct is written for SQL and never read back.
    let bytes = store
        .backend()
        .read("fights", &format!("{kill}.json"))
        .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(
        text.contains("\"taken\":84000,\"mitigated\":85000,\"prevented\":55000"),
        "{text}"
    );
    assert_eq!(
        text.matches("\"mitigated_pct\":").count(),
        card.players.len()
    );
    // Every stored fight was swung at: each card names a friendly player
    // with a taken. (The lake's `cards_without_taken` is SQL's question —
    // "some spec'd player has no stored `taken`" — which a parsed card
    // cannot ask: absent and 0 are one value after `from_json`.)
    for c in store.cards() {
        assert!(
            c.players.iter().any(|p| !p.enemy && p.taken > 0),
            "{}: {:?}",
            c.name,
            c.players
        );
    }

    // The rows tier: one entry per friendly player who was swung at, the
    // record, both drills, and nothing folded (three abilities at most).
    let rows = store.rows(&kill).expect("rows tier");
    let seg = &fights
        .iter()
        .find(|f| f.segment.name == "Taken Test Boss")
        .expect("the pull")
        .segment;
    assert_eq!(
        rows.mitigation
            .iter()
            .map(|m| m.guid.clone())
            .collect::<Vec<_>>(),
        vec![PYRALIS.to_string(), DURGAN.to_string(), ZENLI.to_string()],
        "card order (the Damage view leads)"
    );
    for m in &rows.mitigation {
        let row = seg
            .rows(View::Taken)
            .into_iter()
            .find(|r| r.key == m.guid)
            .expect("a Taken row");
        assert_eq!(
            m.other,
            wowdps_proto::history::TakenOther::default(),
            "{}",
            m.guid
        );
        assert_eq!(m.other.n, 0, "nothing folded on a boss pull");
        assert_eq!(
            m.other_sources,
            wowdps_proto::history::TakenOther::default(),
            "{}: three attackers at most",
            m.guid
        );
        assert_eq!(
            m.taken_spells.iter().map(|r| r.amount).sum::<u64>(),
            row.amount,
            "{}: Σ by-ability = the Taken row",
            m.guid
        );
        assert_eq!(
            m.taken_sources.iter().map(|r| r.amount).sum::<u64>(),
            row.amount,
            "{}: Σ by-attacker = the Taken row",
            m.guid
        );
        assert_eq!(Some(m.record), seg.mitigation(&m.guid));
    }
    let durgan = rows.mitigation.iter().find(|m| m.guid == DURGAN).unwrap();
    assert_eq!(durgan.record.mitigated(), 85_000);
    assert_eq!(durgan.record.prevented(), 55_000);
    assert!(
        durgan
            .taken_sources
            .iter()
            .any(|r| r.key == "Taken Test Boss"),
        "{:?}",
        durgan.taken_sources
    );
    let pyralis = rows.mitigation.iter().find(|m| m.guid == PYRALIS).unwrap();
    assert!(
        pyralis.taken_sources.iter().any(|r| r.key == "Environment"),
        "the fall is an attacker: {:?}",
        pyralis.taken_sources
    );
}

/// A boss hitting one tank with `n` distinct abilities, so the by-ability
/// cap has something to bite. Amount rises with the ability index and every
/// hit carries the fixture's 12 000 partial absorb.
fn many_abilities_log(n: usize) -> String {
    let mut out = String::from(
        "9/3/2026 21:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.7,PROJECT_ID,1\n\
         9/3/2026 21:00:00.100-4  ZONE_CHANGE,2769,\"Sepulcher of the Ashen Vow\",16\n\
         9/3/2026 21:05:00.000-4  ENCOUNTER_START,3145,\"Taken Test Boss\",16,3,2769\n",
    );
    for i in 1..=n {
        out.push_str(&format!(
            "9/3/2026 21:05:{:02}.000-4  SPELL_DAMAGE,Creature-0-4232-2662-31585-215000-0000AB01,\
             \"Taken Test Boss\",0xa48,0x80,{DURGAN},\"Durgan-Nebula-US\",0x511,0x80000000,\
             {},\"Ability {i}\",0x4,{DURGAN},0000000000000000,1128000,1200000,21000,0,14000,28000,\
             0,0,1,60,100,0,-810.12,2148.30,2287,3.1416,650,{},45000,-1,4,0,0,12000,nil,nil,nil,ST\n",
            i,
            380_100 + i,
            1000 * i
        ));
    }
    out.push_str("9/3/2026 21:06:00.000-4  ENCOUNTER_END,3145,\"Taken Test Boss\",16,3,1,60000\n");
    out
}

#[test]
fn the_by_ability_list_is_capped_at_sixteen_and_the_rest_roll_up_exactly() {
    let tmp = Temp::new("cap");
    let (_, facts, fights) = tmp.log("WoWCombatLog-cap.txt", &many_abilities_log(20));
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids: Vec<String> = fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    assert_eq!(ids.len(), 1, "one pull");
    let seg = &fights[0].segment;
    let row = seg
        .rows(View::Taken)
        .into_iter()
        .find(|r| r.key == DURGAN)
        .expect("the tank's Taken row");
    let (live_spells, _) = seg.breakdown(DURGAN, View::Taken);
    assert_eq!(live_spells.len(), 20, "twenty abilities on the live meter");

    let rows = store.rows(&ids[0]).expect("rows tier");
    let m = rows
        .mitigation
        .iter()
        .find(|m| m.guid == DURGAN)
        .expect("the tank's record");
    assert_eq!(m.taken_spells.len(), 16, "TAKEN_SPELLS_CAP");
    assert_eq!(m.other.n, 4, "four abilities folded");
    // The identity the plan states, on all three tallies.
    assert_eq!(
        m.taken_spells.iter().map(|r| r.amount).sum::<u64>() + m.other.amount,
        row.amount,
        "Σ kept + other = the Taken row's amount"
    );
    assert_eq!(
        m.taken_spells.iter().map(|r| r.extra).sum::<u64>() + m.other.extra,
        row.extra,
        "…and its extra (absorbed)"
    );
    assert_eq!(
        m.taken_spells.iter().map(|r| r.count).sum::<u64>() + m.other.count,
        row.count,
        "…and its count"
    );
    // The kept sixteen are the LARGEST, and the fold is the four smallest.
    let smallest: u64 = (1..=4).map(|i| 1000 * i + 12_000).sum();
    assert_eq!(m.other.amount, smallest);
    assert_eq!(m.other.extra, 4 * 12_000);
    assert_eq!(m.other.count, 4);
    assert!(
        m.taken_spells.iter().all(|r| r.amount >= 5_000 + 12_000),
        "{:?}",
        m.taken_spells
    );
    // A drill answers the capped list, not the live twenty.
    let sf = store
        .stored_fight(&ids[0], View::Taken, Some(DURGAN))
        .expect("the card");
    let b = sf.breakdown.expect("the drill");
    assert_eq!(b.by_spell.len(), 16);
    assert_eq!(b.mitigation, seg.mitigation(DURGAN));
    // Three abilities' worth of attackers is one: nothing folded there.
    assert_eq!(m.taken_sources.len(), 1);
    assert_eq!(
        m.other_sources,
        wowdps_proto::history::TakenOther::default()
    );
}

/// `n` distinct creatures each hitting the tank once with the same
/// ability, so the by-attacker cap has something to bite; amount rises
/// with the attacker index, every hit absorbs 12 000.
fn many_attackers_log(n: usize) -> String {
    let mut out = String::from(
        "9/3/2026 21:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.7,PROJECT_ID,1\n\
         9/3/2026 21:00:00.100-4  ZONE_CHANGE,2769,\"Sepulcher of the Ashen Vow\",16\n\
         9/3/2026 21:05:00.000-4  ENCOUNTER_START,3145,\"Taken Test Boss\",16,3,2769\n",
    );
    for i in 1..=n {
        out.push_str(&format!(
            "9/3/2026 21:05:{:02}.000-4  SPELL_DAMAGE,Creature-0-4232-2662-31585-{}-0000AB{:02X},\
             \"Attacker {i}\",0xa48,0x80,{DURGAN},\"Durgan-Nebula-US\",0x511,0x80000000,\
             380100,\"Smash\",0x4,{DURGAN},0000000000000000,1128000,1200000,21000,0,14000,28000,\
             0,0,1,60,100,0,-810.12,2148.30,2287,3.1416,650,{},45000,-1,4,0,0,12000,nil,nil,nil,ST\n",
            i,
            215_000 + i,
            i,
            1000 * i
        ));
    }
    out.push_str("9/3/2026 21:06:00.000-4  ENCOUNTER_END,3145,\"Taken Test Boss\",16,3,1,60000\n");
    out
}

#[test]
fn the_by_attacker_list_is_capped_at_sixteen_and_the_rest_roll_up_exactly() {
    let tmp = Temp::new("cap-sources");
    let (_, facts, fights) = tmp.log("WoWCombatLog-cap-sources.txt", &many_attackers_log(20));
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids: Vec<String> = fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    assert_eq!(ids.len(), 1, "one pull");
    let seg = &fights[0].segment;
    let row = seg
        .rows(View::Taken)
        .into_iter()
        .find(|r| r.key == DURGAN)
        .expect("the tank's Taken row");
    let (live_spells, live_sources) = seg.breakdown(DURGAN, View::Taken);
    assert_eq!(live_sources.len(), 20, "twenty attackers on the live meter");
    assert_eq!(live_spells.len(), 1, "one ability");

    let rows = store.rows(&ids[0]).expect("rows tier");
    let m = rows
        .mitigation
        .iter()
        .find(|m| m.guid == DURGAN)
        .expect("the tank's record");
    assert_eq!(
        m.taken_sources.len(),
        16,
        "TAKEN_SPELLS_CAP bounds attackers too"
    );
    assert_eq!(m.other_sources.n, 4, "four attackers folded");
    // The same identity as the by-ability list, on all three tallies.
    assert_eq!(
        m.taken_sources.iter().map(|r| r.amount).sum::<u64>() + m.other_sources.amount,
        row.amount,
        "Σ kept + other_sources = the Taken row's amount"
    );
    assert_eq!(
        m.taken_sources.iter().map(|r| r.extra).sum::<u64>() + m.other_sources.extra,
        row.extra
    );
    assert_eq!(
        m.taken_sources.iter().map(|r| r.count).sum::<u64>() + m.other_sources.count,
        row.count
    );
    // The kept sixteen are the LARGEST; the fold is the four smallest.
    let smallest: u64 = (1..=4).map(|i| 1000 * i + 12_000).sum();
    assert_eq!(m.other_sources.amount, smallest);
    assert_eq!(m.other_sources.extra, 4 * 12_000);
    assert_eq!(m.other_sources.count, 4);
    assert!(
        m.taken_sources.iter().all(|r| r.amount >= 5_000 + 12_000),
        "{:?}",
        m.taken_sources
    );
    // The one ability is not folded; a drill answers the capped attackers.
    assert_eq!(m.taken_spells.len(), 1);
    assert_eq!(m.other, wowdps_proto::history::TakenOther::default());
    let sf = store
        .stored_fight(&ids[0], View::Taken, Some(DURGAN))
        .expect("the card");
    let b = sf.breakdown.expect("the drill");
    assert_eq!(b.by_target.len(), 16);
    assert_eq!(b.by_spell.len(), 1);
}

#[test]
fn a_stored_taken_drill_equals_the_live_one_on_every_tier() {
    let (store, fights, _, kill) = taken_store(Retention::default());
    let seg = &fights
        .iter()
        .find(|f| f.segment.name == "Taken Test Boss")
        .expect("the pull")
        .segment;
    assert!(store.has_details(&kill), "a kill writes the details tier");

    let expect = |guid: &str| {
        let (by_spell, by_target) = seg.breakdown(guid, View::Taken);
        Breakdown {
            by_spell,
            by_target,
            mitigation: seg.mitigation(guid),
            ..Breakdown::default()
        }
    };
    for guid in [DURGAN, ZENLI, PYRALIS] {
        let sf = store.stored_fight(&kill, View::Taken, Some(guid)).unwrap();
        assert_eq!(sf.tier, 3, "the kill has every tier");
        assert_eq!(
            sf.breakdown.as_ref(),
            Some(&expect(guid)),
            "{guid} at tier 3"
        );
    }
    // No drill, no breakdown — as on every other view.
    assert!(
        store
            .stored_fight(&kill, View::Taken, None)
            .unwrap()
            .breakdown
            .is_none()
    );

    // Demote the details tier (retention's own unlink) and ask again: the
    // Taken drill lives on the ROWS tier, so tier 2 answers identically.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let demoted = Store::open(backend, Retention::default());
    assert!(!demoted.has_details(&kill));
    for guid in [DURGAN, ZENLI, PYRALIS] {
        let sf = demoted
            .stored_fight(&kill, View::Taken, Some(guid))
            .unwrap();
        assert_eq!(sf.tier, 2, "rows only");
        assert_eq!(
            sf.breakdown.as_ref(),
            Some(&expect(guid)),
            "{guid} at tier 2"
        );
        // Damage still needs the details tier, so its drill is gone.
        assert!(
            demoted
                .stored_fight(&kill, View::Damage, Some(guid))
                .unwrap()
                .breakdown
                .is_none()
        );
    }
}

#[test]
fn a_regrade_back_fills_a_pre_2b_record_and_keeps_its_pin() {
    let (store, fights, facts, kill) = taken_store(Retention::default());
    let kill_fight = fights
        .iter()
        .find(|f| f.segment.name == "Taken Test Boss")
        .expect("the pull");
    let file = format!("{kill}.json");
    let fresh_card = String::from_utf8(store.backend().read("fights", &file).unwrap()).unwrap();
    let fresh_rows = String::from_utf8(store.backend().read("rows", &file).unwrap()).unwrap();
    let content_before = store.card(&kill).unwrap().content;

    // Copy the store into a new backend with the kill written the way PR
    // #16 wrote it: the five card measures and the whole `mitigation` array
    // surgically removed, nothing else touched.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "details", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let mut stripped = fresh_card.clone();
    for key in ["taken", "mitigated", "prevented", "dtps", "mitigated_pct"] {
        while let Some(at) = stripped.find(&format!("\"{key}\":")) {
            let end = stripped[at..]
                .find(['}', ','])
                .map(|i| at + i)
                .expect("a value ends");
            let cut = if stripped.as_bytes()[end] == b',' {
                end + 1
            } else {
                end
            };
            stripped.replace_range(at..cut, "");
        }
    }
    // The five keys end each player object, so cutting the last one leaves
    // a dangling comma — a PR #16 writer never wrote one.
    let stripped = stripped.replace(",}", "}");
    for key in ["taken", "mitigated", "prevented", "dtps", "mitigated_pct"] {
        assert!(
            !stripped.contains(&format!("\"{key}\"")),
            "{key}: {stripped}"
        );
    }
    assert!(stripped.len() < fresh_card.len());
    backend.write("fights", &file, stripped.as_bytes()).unwrap();
    let at = fresh_rows
        .find(",\"mitigation\":[")
        .expect("the rows array");
    let end = fresh_rows.rfind('}').expect("the object closes");
    let rows_stripped = format!("{}{}", &fresh_rows[..at], &fresh_rows[end..]);
    assert!(!rows_stripped.contains("\"mitigation\""), "{rows_stripped}");
    backend
        .write("rows", &file, rows_stripped.as_bytes())
        .unwrap();

    let mut reopened = Store::open(backend, Retention::default());
    let old = reopened.card(&kill).expect("the pre-2b card still reads");
    assert_eq!(old.id, kill, "the id is the log + start, never the content");
    for guid in [DURGAN, ZENLI, PYRALIS] {
        let p = player(old, guid);
        assert_eq!((p.taken, p.mitigated, p.prevented), (0, 0, 0), "{guid}");
        assert!(
            close(p.dtps, 0.0) && close(p.mitigated_pct(), 0.0),
            "{guid}"
        );
    }
    // The whole store is un-regraded: no card carries a tank measure.
    assert_eq!(reopened.cards().len(), 1);
    assert!(
        reopened
            .cards()
            .iter()
            .all(|c| c.players.iter().all(|p| p.taken == 0 && p.mitigated == 0))
    );
    assert!(
        reopened
            .stored_fight(&kill, View::Taken, Some(DURGAN))
            .unwrap()
            .breakdown
            .is_none(),
        "a pre-2b rows file has no mitigation to drill"
    );
    // The rows themselves still serve: this is a back-fill, not a repair.
    assert_eq!(
        reopened
            .stored_fight(&kill, View::Taken, None)
            .unwrap()
            .rows,
        kill_fight.segment.rows(View::Taken)
    );

    assert!(reopened.pin(&kill, true));
    assert_eq!(
        reopened.regrade(kill_fight, facts).as_deref(),
        Some(kill.as_str())
    );
    let card = reopened.card(&kill).unwrap();
    assert!(card.pinned, "the pin survived the rewrite");
    assert_eq!(card.id, kill);
    assert_eq!(player(card, DURGAN).taken, 84_000);
    assert_eq!(player(card, DURGAN).mitigated, 85_000);
    assert!(
        reopened
            .cards()
            .iter()
            .all(|c| c.players.iter().any(|p| !p.enemy && p.taken > 0)),
        "nothing left for a regrade to fill"
    );
    let rewritten = String::from_utf8(reopened.backend().read("fights", &file).unwrap()).unwrap();
    assert_eq!(
        rewritten,
        fresh_card.replace("\"pinned\":false", "\"pinned\":true"),
        "byte-for-byte the live write, pin aside"
    );
    assert_eq!(
        String::from_utf8(reopened.backend().read("rows", &file).unwrap()).unwrap(),
        fresh_rows,
        "the rows tier is back to its full shape"
    );
    let b = reopened
        .stored_fight(&kill, View::Taken, Some(DURGAN))
        .unwrap()
        .breakdown
        .expect("the back-filled drill");
    assert_eq!(b.mitigation, kill_fight.segment.mitigation(DURGAN));
    // Every fixture player deals damage, so this card's friendly set —  and
    // so its content id — is unchanged. A log where somebody was only
    // dodged is the case where a regrade MOVES `content` (the next test):
    // the id never moves, the content may.
    assert_eq!(card.content, content_before);
}

/// A pull where one player only ever gets dodged: they deal nothing, heal
/// nothing and die not at all, so before step 2b they were on no view's
/// rows at all. R17 gives them a count-only Taken row.
fn dodged_only_log() -> String {
    format!(
        "9/3/2026 21:00:00.000-4  COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.7,PROJECT_ID,1\n\
         9/3/2026 21:00:00.100-4  ZONE_CHANGE,2769,\"Sepulcher of the Ashen Vow\",16\n\
         9/3/2026 21:05:00.000-4  ENCOUNTER_START,3145,\"Taken Test Boss\",16,3,2769\n\
         9/3/2026 21:05:01.000-4  SPELL_DAMAGE,{PYRALIS},\"Pyralis-Nebula-US\",0x514,0x80000000,\
         Creature-0-4232-2662-31585-215000-0000AB01,\"Taken Test Boss\",0xa48,0x80,133,\"Fireball\",0x4,\
         Creature-0-4232-2662-31585-215000-0000AB01,0000000000000000,235000,300000,0,0,0,0,0,0,0,0,0,0,\
         -812.44,2145.87,2287,4.7123,83,65000,65000,-1,4,0,0,0,nil,nil,ST\n\
         9/3/2026 21:05:02.000-4  SWING_MISSED,Creature-0-4232-2662-31585-215000-0000AB01,\
         \"Taken Test Boss\",0xa48,0x80,{DURGAN},\"Durgan-Nebula-US\",0x511,0x80000000,DODGE,nil\n\
         9/3/2026 21:06:00.000-4  ENCOUNTER_END,3145,\"Taken Test Boss\",16,3,1,60000\n"
    )
}

#[test]
fn a_dodged_only_player_joins_the_card_and_moves_content_but_never_the_id() {
    use wowdps_proto::history::{content_id, fight_id};
    let tmp = Temp::new("dodge");
    let (_, facts, fights) = tmp.log("WoWCombatLog-dodge.txt", &dodged_only_log());
    let mut store = Store::open(MemBackend::new(), Retention::default());
    let ids: Vec<String> = fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    assert_eq!(ids.len(), 1);
    let seg = &fights[0].segment;
    let card = store.card(&ids[0]).unwrap();

    let durgan = player(card, DURGAN);
    assert_eq!(
        (durgan.taken, durgan.damage, durgan.healing, durgan.deaths),
        (0, 0, 0, 0),
        "a dodge is a row of counts alone"
    );
    assert_eq!(durgan.mitigated, 0, "a dodge carries no amount");
    assert_eq!(
        card.players
            .iter()
            .map(|p| p.guid.clone())
            .collect::<Vec<_>>(),
        vec![PYRALIS.to_string(), DURGAN.to_string()],
        "the Taken view adds the dodged player after the dealers"
    );

    // The id is the log identity + the start, so it is what it always was;
    // `content` hashes the FRIENDLY SET, which R17 just grew.
    assert_eq!(card.id, fight_id(facts.id, seg.start_ms, false));
    let dealers_only = content_id(seg.encounter, card.start_utc_ms, [PYRALIS]);
    assert_eq!(
        card.content,
        content_id(seg.encounter, card.start_utc_ms, [PYRALIS, DURGAN])
    );
    assert_ne!(
        card.content, dealers_only,
        "a regrade of a PR #16 card moves its content — never its id"
    );
    // And the dodged player still gets a mitigation entry: a miss alone.
    let rows = store.rows(&ids[0]).unwrap();
    let m = rows
        .mitigation
        .iter()
        .find(|m| m.guid == DURGAN)
        .expect("dodged, therefore swung at");
    assert_eq!(m.record.misses(), 1);
    assert_eq!(m.record.mitigated(), 0);
    assert_eq!(m.other.n, 0);
}

// ---- step 2b: the fixed questions ------------------------------------------------

fn owned_by(name: &str) -> Retention {
    Retention {
        characters: vec![name.to_string()],
        ..Retention::default()
    }
}

fn trend_of(store: &Store<MemBackend>, guid: &str, measure: TrendMeasure) -> Vec<TrendPoint> {
    match store.answer(&HistoryQuery::Trend {
        guid: guid.to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure,
        bucket: TrendBucket::None,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::Trend(points) => points,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_trend_by_dtps_or_mitigated_pct_carries_the_tank_measures() {
    let (store, _, _, kill) = taken_store(owned_by("Durgan-Nebula-US"));

    let dps = trend_of(&store, DURGAN, TrendMeasure::Dps);
    assert_eq!(dps.len(), 1, "one stored pull: {dps:?}");
    assert_eq!(
        (dps[0].amount, dps[0].fight_id.as_str()),
        (71_000, kill.as_str())
    );

    // Dtps: amount = taken, per_sec = the card's dtps.
    let dtps = trend_of(&store, DURGAN, TrendMeasure::Dtps);
    assert_eq!(dtps.len(), 1);
    assert_eq!(dtps[0].amount, 84_000, "the numerator is taken");
    assert!(close(dtps[0].per_sec, 1400.0), "{}", dtps[0].per_sec);
    assert_eq!(dtps[0].duration_ms, 60_000);
    assert_eq!(dtps[0].n, 1);

    // MitigatedPct: amount = mitigated, per_sec = the derived percentage.
    let pct = trend_of(&store, DURGAN, TrendMeasure::MitigatedPct);
    assert_eq!(pct.len(), 1);
    assert_eq!(pct[0].amount, 85_000, "the numerator is mitigated");
    assert!(
        close(pct[0].per_sec, 85_000.0 * 100.0 / 139_000.0),
        "{}",
        pct[0].per_sec
    );
    assert!(pct[0].per_sec > 61.1 && pct[0].per_sec < 61.2);

    // The monk and the mage answer their own rows, not the tank's.
    assert_eq!(
        trend_of(&store, ZENLI, TrendMeasure::Dtps)[0].amount,
        70_200
    );
    assert_eq!(
        trend_of(&store, PYRALIS, TrendMeasure::MitigatedPct)[0].amount,
        26_000
    );

    // A Day bucket folds `per_sec` as a running MEAN of the per-fight
    // values — for MitigatedPct a mean of pcts, never Σ mitigated / Σ swung.
    let day = match store.answer(&HistoryQuery::Trend {
        guid: DURGAN.to_string(),
        spec: None,
        encounter: None,
        difficulty: None,
        measure: TrendMeasure::MitigatedPct,
        bucket: TrendBucket::Day,
        since_utc_ms: None,
        limit: 0,
        local_cutover_hour: None,
    }) {
        HistoryAnswer::Trend(points) => points,
        other => panic!("{other:?}"),
    };
    assert_eq!(day.len(), 1);
    assert!(
        close(day[0].per_sec, pct[0].per_sec),
        "one fight: the mean is it"
    );
    assert_eq!(day[0].amount, 85_000);
}

fn fights_with(store: &Store<MemBackend>, guid: Option<&str>, role: Option<Role>) -> Vec<String> {
    match store.answer(&HistoryQuery::Fights {
        encounter: None,
        difficulty: None,
        guid: guid.map(str::to_string),
        since_utc_ms: None,
        kind: None,
        sort: FightSort::Newest,
        limit: 0,
        after_id: None,
        role,
    }) {
        HistoryAnswer::Fights { cards, .. } => cards.into_iter().map(|c| c.id).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_role_filter_reads_the_subjects_spec_and_no_ops_without_a_subject() {
    // Owner = the Protection Warrior: a Tank filter keeps the fight.
    let (store, _, _, kill) = taken_store(owned_by("Durgan-Nebula-US"));
    assert_eq!(store.owner().map(|(g, _)| g).as_deref(), Some(DURGAN));
    assert_eq!(
        fights_with(&store, None, Some(Role::Tank)),
        vec![kill.clone()]
    );
    assert!(fights_with(&store, None, Some(Role::Dps)).is_empty());
    assert!(fights_with(&store, None, Some(Role::Healer)).is_empty());
    assert_eq!(fights_with(&store, None, None), vec![kill.clone()]);

    // An explicit guid is the subject instead of the owner: the Fire Mage
    // played no fight as a tank, and the Brewmaster played this one.
    assert!(fights_with(&store, Some(PYRALIS), Some(Role::Tank)).is_empty());
    assert_eq!(
        fights_with(&store, Some(PYRALIS), Some(Role::Dps)),
        vec![kill.clone()]
    );
    assert_eq!(
        fights_with(&store, Some(ZENLI), Some(Role::Tank)),
        vec![kill.clone()]
    );

    // A guid nobody on the card carries filters everything out, role or not
    // — the pre-existing `guid` filter, untouched.
    assert!(fights_with(&store, Some("Player-0-nobody"), Some(Role::Tank)).is_empty());
    assert!(fights_with(&store, Some("Player-0-nobody"), None).is_empty());

    // No subject at all (one log, so nothing to intersect, and no guid):
    // the filter is a no-op and every fight still answers.
    let (anon, _, _, kill2) = taken_store(Retention::default());
    assert!(anon.owner().is_none(), "one log cannot name its logger");
    for role in [Role::Tank, Role::Healer, Role::Dps] {
        assert_eq!(
            fights_with(&anon, None, Some(role)),
            vec![kill2.clone()],
            "{role:?} is a no-op without a subject"
        );
    }
}

// ---- step 2b: the protected set --------------------------------------------------

const OWNER: &str = "Player-1-Owner";

#[allow(clippy::too_many_arguments)]
fn synth_card(
    start: i64,
    spec: Spec,
    dps: f64,
    hps: f64,
    taken: u64,
    mitigated: u64,
    duration_ms: i64,
    success: Option<bool>,
    aborted: bool,
) -> FightCard {
    FightCard {
        id: wowdps_proto::history::fight_id(0x1234, start, false),
        log: 0x1234,
        kind: wowdps_proto::history::FightKind::Encounter,
        name: "The Ashen Warden".to_string(),
        encounter: Some(wowdps_model::Encounter {
            id: 3130,
            difficulty: 15,
            group_size: 20,
        }),
        start_local_ms: start,
        start_utc_ms: start,
        duration_ms,
        success,
        aborted,
        players: vec![CardPlayer {
            guid: OWNER.to_string(),
            name: "Ana-Realm".to_string(),
            spec: Some(spec),
            logged: true,
            dps,
            hps,
            taken,
            mitigated,
            ..CardPlayer::default()
        }],
        ..FightCard::default()
    }
}

/// Synthetic cards in a store that keeps one per group, then one real write
/// so retention runs over all of them. Returns the surviving starts.
fn survivors(cards: &[FightCard]) -> Vec<i64> {
    let mut backend = MemBackend::new();
    for c in cards {
        backend
            .write(
                "fights",
                &format!("{}.json", c.id),
                c.to_json().to_line().as_bytes(),
            )
            .unwrap();
    }
    let mut store = Store::open(
        backend,
        Retention {
            keep_per_encounter: 1,
            characters: vec!["Ana-Realm".to_string()],
            ..Retention::default()
        },
    );
    // Any write runs `retain` over every group; the fixture's own pull is
    // a different group and simply rides along.
    let path = Path::new(TAKEN);
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    assert!(fights.iter().filter_map(|f| store.store(f, facts)).count() >= 1);
    let mut out: Vec<i64> = store
        .cards()
        .iter()
        .filter(|c| c.encounter.is_some_and(|e| e.id == 3130))
        .map(|c| c.start_local_ms)
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn the_protected_set_keeps_a_tanks_best_mitigated_pct() {
    // Three kills by the owner, a Protection Warrior. The OLDEST mitigated
    // the most (85 000 of 139 000 = 61 %); the newest is the fastest kill.
    // Neither dps nor hps protects anything here — both are 0.
    let cards = [
        synth_card(
            1_000,
            Spec::ProtectionWarrior,
            0.0,
            0.0,
            84_000,
            85_000,
            90_000,
            Some(true),
            false,
        ),
        synth_card(
            2_000,
            Spec::ProtectionWarrior,
            0.0,
            0.0,
            84_000,
            20_000,
            80_000,
            Some(true),
            false,
        ),
        synth_card(
            3_000,
            Spec::ProtectionWarrior,
            0.0,
            0.0,
            84_000,
            30_000,
            70_000,
            Some(true),
            false,
        ),
    ];
    assert_eq!(
        survivors(&cards),
        vec![1_000, 3_000],
        "the best mitigated_pct (oldest) and the fastest kill (newest)"
    );

    // A wipe is not a personal best: with the best-pct pull aborted, and
    // the middle one merely a wipe, only the fastest kill survives.
    let cards = [
        synth_card(
            1_000,
            Spec::ProtectionWarrior,
            0.0,
            0.0,
            84_000,
            85_000,
            90_000,
            None,
            true,
        ),
        synth_card(
            2_000,
            Spec::ProtectionWarrior,
            0.0,
            0.0,
            84_000,
            80_000,
            80_000,
            Some(false),
            false,
        ),
        synth_card(
            3_000,
            Spec::ProtectionWarrior,
            0.0,
            0.0,
            84_000,
            30_000,
            70_000,
            Some(true),
            false,
        ),
    ];
    assert_eq!(
        survivors(&cards),
        vec![3_000],
        "kills only: an aborted or wiped pull is never a best"
    );

    // The measure is a TANK's: the same numbers on a Fire Mage protect
    // nothing, so only the fastest kill survives.
    let cards = [
        synth_card(
            1_000,
            Spec::Fire,
            0.0,
            0.0,
            84_000,
            85_000,
            90_000,
            Some(true),
            false,
        ),
        synth_card(
            2_000,
            Spec::Fire,
            0.0,
            0.0,
            84_000,
            20_000,
            80_000,
            Some(true),
            false,
        ),
        synth_card(
            3_000,
            Spec::Fire,
            0.0,
            0.0,
            84_000,
            30_000,
            70_000,
            Some(true),
            false,
        ),
    ];
    assert_eq!(
        survivors(&cards),
        vec![3_000],
        "a DPS's mitigation is no best"
    );
}

#[test]
fn a_measure_of_zero_protects_nothing_and_an_aborted_fight_is_never_a_best() {
    // A Fire Mage who never healed: hps is 0.0 on every card. Before the
    // floor, `or_insert` handed "best hps = 0.0" to whichever card came
    // first — the oldest — and protected it forever. The middle card has
    // the highest dps but is ABORTED, so it is no best either. All three
    // are wipes, so nothing is a fastest kill.
    let cards = [
        synth_card(
            1_000,
            Spec::Fire,
            100.0,
            0.0,
            0,
            0,
            90_000,
            Some(false),
            false,
        ),
        synth_card(2_000, Spec::Fire, 500.0, 0.0, 0, 0, 80_000, None, true),
        synth_card(
            3_000,
            Spec::Fire,
            200.0,
            0.0,
            0,
            0,
            70_000,
            Some(false),
            false,
        ),
    ];
    assert_eq!(
        survivors(&cards),
        vec![3_000],
        "only the best dps (which is also the newest) survives"
    );

    // With the same shape but the best dps on the OLDEST card, that card
    // is the one kept — the floor drops zeros, not real numbers, and the
    // cap of one takes both younger pulls.
    let cards = [
        synth_card(
            1_000,
            Spec::Fire,
            900.0,
            0.0,
            0,
            0,
            90_000,
            Some(false),
            false,
        ),
        synth_card(
            2_000,
            Spec::Fire,
            100.0,
            0.0,
            0,
            0,
            80_000,
            Some(false),
            false,
        ),
        synth_card(
            3_000,
            Spec::Fire,
            200.0,
            0.0,
            0,
            0,
            70_000,
            Some(false),
            false,
        ),
    ];
    assert_eq!(survivors(&cards), vec![1_000], "the owner's best dps");
}

// ---- the rows-tier budget, measured -----------------------------------------------

/// Manual measurement, not a gate: how much of a stored rows file the two
/// Taken lists (`taken_spells`, capped at 16, and the uncapped
/// `taken_sources`) take, per stored fight of a REAL log — the numbers
/// behind the step 2b plan's "Rows-tier measurement" section. Imports the
/// log into a `MemBackend` store, never the user's own. Run with:
/// `WOWDPS_REAL_LOG=/path/to/WoWCombatLog-*.txt cargo test --release -p wowdps-daemon --test taken -- --ignored real_log --nocapture`
#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a real combat log"]
fn real_log_rows_tier_taken_lists_measured() {
    use std::io::BufRead;

    let path =
        std::path::PathBuf::from(std::env::var("WOWDPS_REAL_LOG").expect("set WOWDPS_REAL_LOG"));
    let facts = LogFacts::read(&path);
    let mut store = Store::open(MemBackend::new(), Retention::default());

    // Stream the log in chunks: a raid night is > 1 GB, and every closed
    // fight is stored (and released) as its chunk closes it.
    let mut engine = Engine::new();
    let mut events = Vec::new();
    engine.on_tail(TailEvent::Switched(path.clone()), &mut events);
    let mut stored = Vec::new();
    let mut close = |engine: &Engine, events: &mut Vec<EngineEvent>, stored: &mut Vec<String>| {
        for e in events.drain(..) {
            if let EngineEvent::Closed(id) = e
                && let Some(f) = engine.take_closed(id)
                && let Some(id) = store.store(&f, facts)
            {
                stored.push(id);
            }
        }
    };
    let reader = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
    let mut chunk = Vec::with_capacity(50_000);
    for line in reader.lines().map_while(Result::ok) {
        chunk.push(line);
        if chunk.len() == 50_000 {
            engine.on_tail(TailEvent::Lines(std::mem::take(&mut chunk)), &mut events);
            close(&engine, &mut events, &mut stored);
        }
    }
    engine.on_tail(TailEvent::Lines(chunk), &mut events);
    engine.on_tail(TailEvent::CaughtUp, &mut events);
    close(&engine, &mut events, &mut stored);

    println!(
        "{:<10} {:>4} {:>9} {:>9} {:>9} {:>5} {:>5} {:>5} {:>5} {:>7}  name",
        "kind", "N", "rows B", "spells B", "srcs B", "Σsp", "Σoth", "Σsrc", "Σoth2", "max"
    );
    let mut worst_growth = (0u64, String::new());
    for id in &stored {
        let card = store.card(id).unwrap();
        let bytes = store.backend().read("rows", &format!("{id}.json")).unwrap();
        let rows = store.rows(id).unwrap();
        // The two lists' share of the file: each list's bytes are what its
        // player's JSON loses when that list is emptied.
        let without = |m: &wowdps_proto::history::PlayerMitigation, spells: bool| -> usize {
            let full = m.to_json().to_line().len();
            let mut bare = m.clone();
            if spells {
                bare.taken_spells.clear();
            } else {
                bare.taken_sources.clear();
            }
            full - bare.to_json().to_line().len()
        };
        let spells_bytes: usize = rows.mitigation.iter().map(|m| without(m, true)).sum();
        let sources_bytes: usize = rows.mitigation.iter().map(|m| without(m, false)).sum();
        let sum_spells: usize = rows.mitigation.iter().map(|m| m.taken_spells.len()).sum();
        let sum_other: u32 = rows.mitigation.iter().map(|m| m.other.n).sum();
        let sum_sources: usize = rows.mitigation.iter().map(|m| m.taken_sources.len()).sum();
        let sum_other_sources: u32 = rows.mitigation.iter().map(|m| m.other_sources.n).sum();
        let max_spells = rows
            .mitigation
            .iter()
            .map(|m| m.taken_spells.len())
            .max()
            .unwrap_or(0);
        let max_sources = rows
            .mitigation
            .iter()
            .map(|m| m.taken_sources.len())
            .max()
            .unwrap_or(0);
        let growth = (spells_bytes + sources_bytes) as u64;
        if growth > worst_growth.0 {
            worst_growth = (growth, format!("{:?} {}", card.kind, card.name));
        }
        println!(
            "{:<10} {:>4} {:>9} {:>9} {:>9} {:>5} {:>5} {:>5} {:>5} {:>3}/{:<3}  {}",
            format!("{:?}", card.kind),
            rows.mitigation.len(),
            bytes.len(),
            spells_bytes,
            sources_bytes,
            sum_spells,
            sum_other,
            sum_sources,
            sum_other_sources,
            max_spells,
            max_sources,
            card.name
        );
    }
    println!(
        "{} fights stored; largest growth from the two lists: {} B ({})",
        stored.len(),
        worst_growth.0,
        worst_growth.1
    );
}
