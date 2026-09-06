//! R18 through the history store (step 4b, v25): the card carries the
//! active-mitigation union and the four externals scalars per player
//! (`am_uptime_pct` derived on write); the rows tier carries the uptime
//! rollup keyed by target (`uptime[]`) and the coarse 10 s taken / healing
//! series with the one merged mark list (`coarse[]`); the stored Taken
//! drill answers with the coarse timeline again, the Healing drill keeps
//! the details tier's 1 s series on tier 3 and falls back to `heal10` on
//! tier 2; `stored_fight` hands the drilled player's uptime back in both
//! halves; `stored_fight` == `derived_fight`; `Trend { AmUptime }` reads
//! the percentage; and a regrade back-fills a PR #23-shaped record, pin
//! kept. Numbers are `crates/core/fixtures/spans.expected.md`'s.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::path::{Path, PathBuf};

use wowdps_core::tail::TailEvent;
use wowdps_daemon::engine::{Engine, EngineEvent};
use wowdps_daemon::history::{
    Backend, ClosedFight, DirBackend, LogFacts, MemBackend, Retention, Store,
};
use wowdps_daemon::mock::MockDaemon;
use wowdps_model::{MarkKind, View};
use wowdps_proto::history::{COARSE_BUCKET_MS, CardPlayer, FightCard, FightDetails, FightKind};
use wowdps_proto::{
    HistoryAnswer, HistoryQuery, StoredFight, TrendBucket, TrendMeasure, TrendPoint,
};

const SPANS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/fixtures/spans.txt");

/// spans.txt's roster.
const WARRIOR: &str = "Player-1168-0A1B2C31";
const PRIEST: &str = "Player-1168-0A1B2C32";
const EVOKER: &str = "Player-1168-0A1B2C33";
const MAGE: &str = "Player-1168-0A1B2C34";
const ROSTER: [&str; 4] = [WARRIOR, PRIEST, EVOKER, MAGE];

const BOSS: &str = "Spans Test Boss";
const KILL_MS: i64 = 60_000;

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

fn with_trash() -> Retention {
    Retention {
        store_trash: true,
        ..Retention::default()
    }
}

/// spans.txt with one more line: the boar's swing again ten minutes on,
/// past the R7 gap, so the trash tail CLOSES (a finished log's last trash
/// is live at EOF, never a `Closed` fight) — the Trash card's clamped
/// union then reaches the store.
fn spans_log() -> (Temp, PathBuf) {
    let tmp = Temp::new("log");
    let text = std::fs::read_to_string(SPANS).unwrap();
    let boar = text
        .lines()
        .find(|l| l.contains("20:06:10.000-4  SWING_DAMAGE,Creature"))
        .expect("the boar's swing");
    let later = boar.replace("20:06:10.000-4", "20:16:10.000-4");
    let path = tmp.0.join("WoWCombatLog-090526.txt");
    std::fs::write(&path, format!("{text}{later}\n")).unwrap();
    (tmp, path)
}

fn stored(
    path: &Path,
    cfg: Retention,
) -> (Store<MemBackend>, Vec<ClosedFight>, LogFacts, Vec<String>) {
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    let mut store = Store::open(MemBackend::new(), cfg);
    let ids: Vec<String> = fights
        .iter()
        .filter_map(|f| store.store(f, facts))
        .collect();
    assert!(!ids.is_empty(), "{} stores something", path.display());
    (store, fights, facts, ids)
}

/// spans.txt: the kill and the trash tail, both stored.
fn spans_store() -> (
    Temp,
    Store<MemBackend>,
    Vec<ClosedFight>,
    LogFacts,
    String,
    String,
) {
    let (tmp, path) = spans_log();
    let (store, fights, facts, ids) = stored(&path, with_trash());
    assert_eq!(ids.len(), 2, "the kill and the trash: {ids:?}");
    let kill = ids
        .iter()
        .find(|id| store.card(id).unwrap().name == BOSS)
        .expect("the kill")
        .clone();
    let trash = ids
        .iter()
        .find(|id| store.card(id).unwrap().kind == FightKind::Trash)
        .expect("the trash")
        .clone();
    (tmp, store, fights, facts, kill, trash)
}

fn fight<'a>(fights: &'a [ClosedFight], name: &str) -> &'a ClosedFight {
    fights
        .iter()
        .find(|f| f.segment.name == name)
        .unwrap_or_else(|| panic!("{name} closed"))
}

fn player<'a>(card: &'a FightCard, guid: &str) -> &'a CardPlayer {
    card.players
        .iter()
        .find(|p| p.guid == guid)
        .unwrap_or_else(|| panic!("{guid} on the card: {:?}", card.players))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// (am_uptime_ms, externals_given, given_ms, externals_received, received_ms).
fn scalars(p: &CardPlayer) -> (u64, u32, u64, u32, u64) {
    (
        p.am_uptime_ms,
        p.externals_given,
        p.externals_given_ms,
        p.externals_received,
        p.externals_received_ms,
    )
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
fn the_card_carries_the_am_union_and_the_externals_scalars() {
    let (_tmp, store, fights, _, kill, trash) = spans_store();
    let card = store.card(&kill).unwrap();
    assert_eq!(card.duration_ms, KILL_MS);
    assert_eq!(card.players.len(), 4, "every roster player has a row");
    // The goldens (spans.expected.tsv): the Warrior's union is 27 s of 60
    // (Shield Block × Shield Wall overlapping counted once), three
    // externals received for 58 s; the Priest gives three (38 s) and
    // receives one (Power Infusion, 40 s); the Mage gives three (120 s —
    // one Time Warp on three targets) and receives two (60 s); the Evoker
    // gives nothing an external counts and receives nothing.
    assert_eq!(scalars(player(card, WARRIOR)), (27_000, 0, 0, 3, 58_000));
    assert_eq!(scalars(player(card, PRIEST)), (0, 3, 38_000, 1, 40_000));
    assert_eq!(scalars(player(card, MAGE)), (0, 3, 120_000, 2, 60_000));
    assert_eq!(scalars(player(card, EVOKER)), (0, 0, 0, 0, 0));
    // The pct is derived from the stored ms and the card's duration.
    assert!(close(
        player(card, WARRIOR).am_uptime_pct(card.duration_ms),
        45.0
    ));
    assert!(close(
        player(card, PRIEST).am_uptime_pct(card.duration_ms),
        0.0
    ));
    // Every scalar is the engine's own answer.
    let seg = &fight(&fights, BOSS).segment;
    for guid in ROSTER {
        let p = player(card, guid);
        assert_eq!(
            i64::try_from(p.am_uptime_ms).unwrap(),
            seg.am_uptime_ms(guid),
            "{guid}"
        );
        let (g, g_ms) = seg.externals_given(guid);
        let (r, r_ms) = seg.externals_received(guid);
        assert_eq!(
            (
                p.externals_given,
                p.externals_given_ms,
                p.externals_received,
                p.externals_received_ms
            ),
            (g, g_ms as u64, r, r_ms as u64),
            "{guid}"
        );
    }
    // Trash: the union is clamped at the R7 clock — 5 s of the 8 s pull.
    let tail = store.card(&trash).unwrap();
    assert_eq!(tail.duration_ms, 8_000);
    assert_eq!(player(tail, WARRIOR).am_uptime_ms, 5_000);
    assert!(close(
        player(tail, WARRIOR).am_uptime_pct(tail.duration_ms),
        62.5
    ));
}

#[test]
fn the_rows_tier_carries_the_uptime_rollup_and_the_coarse_series() {
    let (_tmp, store, fights, _, kill, _) = spans_store();
    let seg = &fight(&fights, BOSS).segment;
    let rows = store.rows(&kill).expect("the rows tier");

    // `uptime[]`: one block per friendly player with any cell, keyed by
    // TARGET, the engine's cells verbatim. The Evoker wears no aura.
    let block = |guid: &str| rows.uptime.iter().find(|u| u.guid == guid);
    for guid in [WARRIOR, PRIEST, MAGE] {
        let b = block(guid).unwrap_or_else(|| panic!("{guid} has a block"));
        assert_eq!(b.cells, seg.uptime(guid), "{guid}: the engine's cells");
        assert!(!b.cells.is_empty());
    }
    assert!(block(EVOKER).is_none(), "no cell on the Evoker, no block");
    // The Evoker's support uptime (48 s, the golden) is NOT stored: it is
    // Σ of the `support_buff` cells across the other blocks with them as
    // `src`.
    let support_ms: i64 = rows
        .uptime
        .iter()
        .flat_map(|u| u.cells.iter())
        .filter(|c| c.kind == MarkKind::SupportBuff && c.src == EVOKER)
        .map(|c| c.total_ms)
        .sum();
    assert_eq!(support_ms, 48_000);
    // Externals given by the Priest: Σ of the `external` cells they cast
    // on other targets = the card's `externals_given_ms`.
    let priest_ext: i64 = rows
        .uptime
        .iter()
        .flat_map(|u| u.cells.iter())
        .filter(|c| c.kind == MarkKind::External && c.src == PRIEST)
        .map(|c| c.total_ms)
        .sum();
    assert_eq!(priest_ext, 38_000);

    // `coarse[]`: the Warrior took 22 000 in the first 10 s (the golden
    // `taken10_0`); their marks are the eight R18 spans plus the one R12
    // trinket proc — exactly `timeline(guid).marks`.
    let w = rows
        .coarse
        .iter()
        .find(|c| c.guid == WARRIOR)
        .expect("the Warrior's coarse block");
    assert_eq!(w.taken10[0], 22_000);
    assert_eq!(w.taken10, seg.taken_timeline(WARRIOR).coarsen(10).buckets);
    assert_eq!(w.heal10, seg.heal_timeline(WARRIOR).coarsen(10).buckets);
    assert_eq!(w.marks, seg.timeline(WARRIOR).marks);
    assert_eq!(
        w.marks
            .iter()
            .filter(|m| m.kind != MarkKind::TrinketProc)
            .count(),
        8,
        "eight spans on the tank"
    );
    assert_eq!(
        w.marks
            .iter()
            .filter(|m| m.kind == MarkKind::TrinketProc)
            .count(),
        1,
        "R12 untouched"
    );
    assert_eq!(w.marks.len(), seg.timeline(WARRIOR).marks.len());
    // Σ over the coarse buckets is the Taken row.
    assert_eq!(
        w.taken10.iter().sum::<u64>(),
        player(store.card(&kill).unwrap(), WARRIOR).taken
    );
    // The Priest healed: a heal10 series (47 000 total) and one span.
    let h = rows
        .coarse
        .iter()
        .find(|c| c.guid == PRIEST)
        .expect("the Priest's coarse block");
    assert_eq!(h.heal10.iter().sum::<u64>(), 47_000);
    assert_eq!(h.marks.len(), 1);
    // Every friendly player with a bucket or a mark has a block, no one
    // else: the Evoker neither took, healed nor wore anything.
    for guid in ROSTER {
        let t = seg.taken_timeline(guid).coarsen(10).buckets;
        let hl = seg.heal_timeline(guid).coarsen(10).buckets;
        let any = t.iter().any(|b| *b != 0)
            || hl.iter().any(|b| *b != 0)
            || !seg.timeline(guid).marks.is_empty();
        assert_eq!(
            rows.coarse.iter().any(|c| c.guid == guid),
            any,
            "{guid}: block iff anything to carry"
        );
    }
    assert!(!rows.coarse.iter().any(|c| c.guid == EVOKER));
}

#[test]
fn a_stored_fight_answers_the_coarse_taken_drill_and_both_uptime_halves() {
    let (_tmp, store, fights, facts, kill, trash) = spans_store();
    let kill_fight = fight(&fights, BOSS);
    let seg = &kill_fight.segment;

    // The Taken drill's timeline is the coarse series with the marks.
    let sf = store
        .stored_fight(&kill, View::Taken, Some(WARRIOR))
        .unwrap();
    assert_eq!(sf.tier, 3);
    let bd = sf.breakdown.as_ref().expect("the Taken drill");
    let tl = bd.timeline.as_ref().expect("a timeline again (4b)");
    assert_eq!(tl.bucket_ms, COARSE_BUCKET_MS);
    assert_eq!(tl.bucket_ms, 10_000);
    assert_eq!(*tl, seg.taken_timeline(WARRIOR).coarsen(10));
    assert_eq!(tl.buckets[0], 22_000);
    assert!(bd.mitigation.is_some(), "the 2b record still rides");
    // The Healing drill on tier 3 keeps the details tier's 1 s series.
    let heal = store
        .stored_fight(&kill, View::Healing, Some(PRIEST))
        .unwrap();
    let htl = heal.breakdown.unwrap().timeline.expect("the 1 s series");
    assert_eq!(htl.bucket_ms, 1_000);
    assert_eq!(htl, seg.heal_timeline(PRIEST));
    // The Damage drill is untouched.
    let dmg = store.stored_fight(&kill, View::Damage, Some(MAGE)).unwrap();
    assert_eq!(dmg.breakdown.unwrap().timeline.unwrap(), seg.timeline(MAGE));

    // `uptime`: the Priest's own cells first (they are the target — Power
    // Infusion from the Mage), then the cells they cast on others in
    // roster order: Pain Suppression on the Warrior, Power Infusion on the
    // Mage.
    let own = seg.uptime(PRIEST);
    assert!(!own.is_empty());
    let priest = store
        .stored_fight(&kill, View::Damage, Some(PRIEST))
        .unwrap()
        .uptime;
    assert_eq!(
        priest
            .iter()
            .take(own.len())
            .map(|u| (u.target.as_str(), &u.cell))
            .collect::<Vec<_>>(),
        own.iter().map(|c| (PRIEST, c)).collect::<Vec<_>>(),
        "own block first, engine order"
    );
    let cast: Vec<(&str, &str, MarkKind)> = priest[own.len()..]
        .iter()
        .map(|u| (u.target.as_str(), u.cell.label.as_str(), u.cell.kind))
        .collect();
    assert!(
        cast.contains(&(WARRIOR, "Pain Suppression", MarkKind::External)),
        "{cast:?}"
    );
    assert!(
        cast.contains(&(MAGE, "Power Infusion", MarkKind::External)),
        "{cast:?}"
    );
    assert!(
        priest[own.len()..]
            .iter()
            .all(|u| u.cell.src == PRIEST && u.target != PRIEST),
        "every cast-side cell is the Priest's, on someone else"
    );
    // The cast side follows the CARD's roster order (the Damage view by
    // amount: the Mage's 192 000 before the Warrior's 81 000), so the
    // Power Infusion on the Mage precedes the Pain Suppression on the
    // Warrior.
    let roster: Vec<&str> = store
        .card(&kill)
        .unwrap()
        .players
        .iter()
        .map(|p| p.guid.as_str())
        .collect();
    assert_eq!(roster, [MAGE, WARRIOR, EVOKER, PRIEST]);
    let pos = |t: &str| cast.iter().position(|c| c.0 == t).unwrap();
    assert!(pos(MAGE) < pos(WARRIOR));
    let targets: Vec<&str> = cast.iter().map(|c| c.0).collect();
    let mut sorted = targets.clone();
    sorted.sort_by_key(|t| roster.iter().position(|r| r == t));
    assert_eq!(targets, sorted, "roster order");
    // A self-cast appears once: the Priest's own block holds nothing they
    // cast on themself in this fixture, so the two halves are disjoint.
    assert_eq!(
        priest.len(),
        own.len()
            + rows_cells_by_src(&store, &kill, PRIEST)
                .iter()
                .filter(|(t, _)| *t != PRIEST)
                .count()
    );
    // Σ of the Priest's cast-side `external` cells = the card's
    // `externals_given_ms`.
    let given_ms: i64 = priest[own.len()..]
        .iter()
        .filter(|u| u.cell.kind == MarkKind::External)
        .map(|u| u.cell.total_ms)
        .sum();
    assert_eq!(given_ms, 38_000);

    // The Warrior: target-side cells (their whole block), Shield Block
    // among them with themself as caster — once.
    let warrior = store
        .stored_fight(&kill, View::Taken, Some(WARRIOR))
        .unwrap()
        .uptime;
    let w_own = seg.uptime(WARRIOR);
    assert_eq!(warrior.len(), w_own.len(), "the tank cast on no one else");
    assert!(warrior.iter().all(|u| u.target == WARRIOR));
    assert_eq!(
        warrior
            .iter()
            .filter(|u| u.cell.label == "Shield Block")
            .count(),
        1,
        "a self-cast appears once"
    );
    assert!(
        warrior
            .iter()
            .any(|u| u.cell.kind == MarkKind::ActiveMitigation && u.cell.src == WARRIOR)
    );
    // The Evoker: no own block; every cell is a `support_buff` they cast.
    let evoker = store
        .stored_fight(&kill, View::Damage, Some(EVOKER))
        .unwrap()
        .uptime;
    assert!(!evoker.is_empty());
    assert!(evoker.iter().all(|u| u.cell.kind == MarkKind::SupportBuff
        && u.cell.src == EVOKER
        && u.target != EVOKER));
    assert_eq!(evoker.iter().map(|u| u.cell.total_ms).sum::<i64>(), 48_000);
    // Without a drill: empty.
    assert!(
        store
            .stored_fight(&kill, View::Damage, None)
            .unwrap()
            .uptime
            .is_empty()
    );

    // `derived_fight` builds the same from the segment — every view, every
    // player, drilled and not.
    for view in [View::Damage, View::Healing, View::Taken, View::Deaths] {
        for drill in ROSTER.iter().map(|g| Some(*g)).chain([None]) {
            let a = store.stored_fight(&kill, view, drill).unwrap();
            let b = store.derived_fight(kill_fight, facts, view, drill);
            assert_eq!(a, b, "{view:?} {drill:?}");
        }
    }
    let trash_fight = fights
        .iter()
        .find(|f| f.segment.kind == wowdps_core::meter::SegmentKind::Trash)
        .expect("the trash tail closed");
    let a = store
        .stored_fight(&trash, View::Taken, Some(WARRIOR))
        .unwrap();
    let b = store.derived_fight(trash_fight, facts, View::Taken, Some(WARRIOR));
    // A Trash fight stores no details tier (tier 2) while `derived_fight`
    // always has the parse in hand (tier 3) — pre-existing; the Taken
    // drill answers from the rows tier and is identical either way.
    assert_eq!((a.tier, b.tier), (2, 3));
    assert_eq!(
        StoredFight {
            tier: 3,
            ..a.clone()
        },
        b
    );
    assert_eq!(a.breakdown.unwrap().timeline.unwrap().buckets, vec![1_500]);
}

/// Every (target, cell) on the rows tier whose `src` is `guid`.
fn rows_cells_by_src(
    store: &Store<MemBackend>,
    id: &str,
    guid: &str,
) -> Vec<(String, wowdps_model::UptimeCell)> {
    store
        .rows(id)
        .unwrap()
        .uptime
        .iter()
        .flat_map(|u| {
            u.cells
                .iter()
                .filter(|c| c.src == guid)
                .map(|c| (u.guid.clone(), c.clone()))
        })
        .collect()
}

#[test]
fn a_tier_2_healing_drill_falls_back_to_the_coarse_series() {
    let (_tmp, store, fights, _, kill, _) = spans_store();
    let seg = &fight(&fights, BOSS).segment;
    // A store with the details tier gone (retention demoted it): fights +
    // rows copied, nothing else.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let demoted = Store::open(backend, with_trash());
    let sf = demoted
        .stored_fight(&kill, View::Healing, Some(PRIEST))
        .unwrap();
    assert_eq!(sf.tier, 2);
    let bd = sf.breakdown.expect("the coarse series answers on tier 2");
    assert!(
        bd.by_spell.is_empty() && bd.by_target.is_empty(),
        "the lists are gone"
    );
    let tl = bd.timeline.expect("heal10");
    assert_eq!(tl, seg.heal_timeline(PRIEST).coarsen(10));
    assert_eq!(tl.bucket_ms, COARSE_BUCKET_MS);
    // The Taken drill is the same on either tier.
    assert_eq!(
        demoted
            .stored_fight(&kill, View::Taken, Some(WARRIOR))
            .unwrap()
            .breakdown,
        store
            .stored_fight(&kill, View::Taken, Some(WARRIOR))
            .unwrap()
            .breakdown
    );
    // A player with no coarse block has no Healing drill on tier 2.
    assert!(
        demoted
            .stored_fight(&kill, View::Healing, Some(EVOKER))
            .unwrap()
            .breakdown
            .is_none()
    );
    // The Damage drill needs the details tier.
    assert!(
        demoted
            .stored_fight(&kill, View::Damage, Some(MAGE))
            .unwrap()
            .breakdown
            .is_none()
    );
    // And the uptime still answers off the rows tier.
    assert_eq!(
        demoted
            .stored_fight(&kill, View::Damage, Some(PRIEST))
            .unwrap()
            .uptime,
        store
            .stored_fight(&kill, View::Damage, Some(PRIEST))
            .unwrap()
            .uptime
    );
}

#[test]
fn a_present_details_tier_that_lacks_the_player_answers_no_healing_drill() {
    // Review S3: the coarse `heal10` fallback is for tier 2 ONLY — the
    // details tier absent. A details file that exists but has no entry for
    // the drilled player answers None, exactly as before step 4b; the
    // coarse block must not stand in for a present-but-silent details doc.
    let (_tmp, store, _, _, kill, _) = spans_store();
    let file = format!("{kill}.json");
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "details", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let doc = String::from_utf8(store.backend().read("details", &file).unwrap()).unwrap();
    let mut details = FightDetails::from_json(&wowdps_proto::json::parse(&doc).unwrap())
        .expect("the details doc parses");
    let before = details.players.len();
    details.players.retain(|p| p.guid != PRIEST);
    assert_eq!(details.players.len() + 1, before, "the Priest was there");
    backend
        .write("details", &file, details.to_json().to_line().as_bytes())
        .unwrap();
    let silent = Store::open(backend, with_trash());
    let sf = silent
        .stored_fight(&kill, View::Healing, Some(PRIEST))
        .unwrap();
    assert_eq!(sf.tier, 3, "the details tier is present");
    assert!(
        sf.breakdown.is_none(),
        "no coarse fallback while details exist: {:?}",
        sf.breakdown
    );
    // The coarse block itself is still there — the Taken drill proves it.
    assert!(
        silent
            .stored_fight(&kill, View::Taken, Some(WARRIOR))
            .unwrap()
            .breakdown
            .and_then(|b| b.timeline)
            .is_some()
    );
    // And a player the trimmed file still holds keeps the 1 s series.
    let tl = silent
        .stored_fight(&kill, View::Healing, Some(EVOKER))
        .unwrap()
        .breakdown
        .and_then(|b| b.timeline)
        .expect("tier 3 heal timeline");
    assert_ne!(tl.bucket_ms, COARSE_BUCKET_MS);
}

#[test]
fn a_trend_by_am_uptime_reads_the_percentage() {
    let (_tmp, store, _, _, kill, trash) = spans_store();
    let points = trend_of(&store, WARRIOR, TrendMeasure::AmUptime);
    let k = points
        .iter()
        .find(|p| p.fight_id == kill)
        .expect("the kill");
    assert_eq!(k.amount, 27_000);
    assert!(close(k.per_sec, 45.0), "{}", k.per_sec);
    assert_eq!(k.duration_ms, KILL_MS);
    let t = points
        .iter()
        .find(|p| p.fight_id == trash)
        .expect("the trash");
    assert_eq!(t.amount, 5_000);
    assert!(close(t.per_sec, 62.5));
    // A healer trends 0 %.
    let h = trend_of(&store, PRIEST, TrendMeasure::AmUptime);
    assert!(h.iter().all(|p| p.amount == 0 && p.per_sec == 0.0));
}

/// Cut every `"key":<scalar>` (with its comma) out of a one-line card.
fn strip_keys(doc: &str, keys: &[&str]) -> String {
    let mut stripped = doc.to_string();
    for key in keys {
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
    stripped.replace(",}", "}")
}

/// Cut a top-level `,"key":[…]` array out of a one-line rows document.
fn strip_array(doc: &str, key: &str) -> String {
    let needle = format!(",\"{key}\":[");
    let at = doc
        .find(&needle)
        .unwrap_or_else(|| panic!("{key} in {doc}"));
    // The array ends where the next top-level key begins or the object
    // closes: scan with a depth counter.
    let bytes = doc.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut end = None;
    for (i, &b) in bytes.iter().enumerate().skip(at + needle.len() - 1) {
        match b {
            b'"' if !in_str => in_str = true,
            b'"' if in_str && bytes[i - 1] != b'\\' => in_str = false,
            b'[' if !in_str => depth += 1,
            b']' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.expect("the array closes");
    format!("{}{}", &doc[..at], &doc[end..])
}

const CARD_KEYS: [&str; 6] = [
    "am_uptime_ms",
    "am_uptime_pct",
    "externals_given",
    "externals_given_ms",
    "externals_received",
    "externals_received_ms",
];

#[test]
fn a_regrade_back_fills_a_pre_4b_record_and_keeps_its_pin() {
    let (_tmp, store, fights, facts, kill, _) = spans_store();
    let kill_fight = fight(&fights, BOSS);
    let file = format!("{kill}.json");
    let fresh_card = String::from_utf8(store.backend().read("fights", &file).unwrap()).unwrap();
    let fresh_rows = String::from_utf8(store.backend().read("rows", &file).unwrap()).unwrap();
    for key in CARD_KEYS {
        assert!(fresh_card.contains(&format!("\"{key}\":")), "{key} written");
    }
    assert!(fresh_rows.contains(",\"uptime\":["));
    assert!(fresh_rows.contains(",\"coarse\":["));
    assert!(
        fresh_rows.contains("\"kind\":\"active_mitigation\""),
        "kinds by NAME"
    );

    // The kill written the way PR #23 wrote it: the six card keys and the
    // two rows arrays surgically removed, nothing else touched.
    let mut backend = MemBackend::new();
    for dir in ["fights", "rows", "details", "loadouts"] {
        for name in store.backend().list(dir) {
            backend
                .write(dir, &name, &store.backend().read(dir, &name).unwrap())
                .unwrap();
        }
    }
    let stripped = strip_keys(&fresh_card, &CARD_KEYS);
    for key in CARD_KEYS {
        assert!(
            !stripped.contains(&format!("\"{key}\"")),
            "{key}: {stripped}"
        );
    }
    assert!(
        stripped.contains("\"mitigated_pct\":"),
        "v22 keys untouched"
    );
    assert!(
        stripped.contains("\"effective_dps\":"),
        "v23 keys untouched"
    );
    backend.write("fights", &file, stripped.as_bytes()).unwrap();
    let rows_stripped = strip_array(&strip_array(&fresh_rows, "uptime"), "coarse");
    assert!(!rows_stripped.contains("\"uptime\""), "{rows_stripped}");
    assert!(!rows_stripped.contains("\"coarse\""), "{rows_stripped}");
    assert!(rows_stripped.contains("\"mitigation\":["));
    assert!(rows_stripped.contains("\"support\":["));
    backend
        .write("rows", &file, rows_stripped.as_bytes())
        .unwrap();

    let mut reopened = Store::open(backend, with_trash());
    assert_eq!(reopened.corrupt(), 0);
    let old = reopened.card(&kill).expect("the pre-4b card still reads");
    assert_eq!(old.id, kill);
    for guid in ROSTER {
        assert_eq!(scalars(player(old, guid)), (0, 0, 0, 0, 0), "{guid}: zeros");
        assert_eq!(player(old, guid).am_uptime_pct(old.duration_ms), 0.0);
    }
    let sf = reopened
        .stored_fight(&kill, View::Taken, Some(WARRIOR))
        .unwrap();
    assert!(sf.uptime.is_empty(), "a pre-4b rows file has no cells");
    let bd = sf.breakdown.expect("the 2b drill still serves");
    assert!(bd.timeline.is_none(), "no coarse block, no timeline");
    assert!(bd.mitigation.is_some());
    assert_eq!(
        trend_of(&reopened, WARRIOR, TrendMeasure::AmUptime)
            .iter()
            .find(|p| p.fight_id == kill)
            .map(|p| (p.amount, p.per_sec)),
        Some((0, 0.0)),
        "a pre-4b card trends 0 %"
    );

    assert!(reopened.pin(&kill, true));
    assert_eq!(
        reopened.regrade(kill_fight, facts).as_deref(),
        Some(kill.as_str())
    );
    let card = reopened.card(&kill).unwrap();
    assert!(card.pinned, "the pin survived the rewrite");
    assert_eq!(scalars(player(card, WARRIOR)), (27_000, 0, 0, 3, 58_000));
    assert_eq!(scalars(player(card, MAGE)), (0, 3, 120_000, 2, 60_000));
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
    let sf = reopened
        .stored_fight(&kill, View::Taken, Some(WARRIOR))
        .unwrap();
    assert_eq!(sf.breakdown.unwrap().timeline.unwrap().buckets[0], 22_000);
    assert!(!sf.uptime.is_empty());
    assert!(close(
        trend_of(&reopened, WARRIOR, TrendMeasure::AmUptime)
            .iter()
            .find(|p| p.fight_id == kill)
            .unwrap()
            .per_sec,
        45.0
    ));
}

/// A scratch directory under the system temp dir, removed on drop — one
/// per call (tests run in parallel and each wants its own).
struct Temp(PathBuf);

static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

impl Temp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "wowdps-spans-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_real_store_round_trips_the_scalars_the_cells_and_the_series() {
    // Write spans.txt through a directory-backed store, reopen it from the
    // files alone, and compare with the in-memory answers: what the JSON
    // carries is what the daemon serves after a restart.
    let tmp = Temp::new("roundtrip");
    let (_log, path) = spans_log();
    let path = path.as_path();
    let facts = LogFacts::read(path);
    let fights = closed_fights(path);
    let mut disk = Store::open(DirBackend::new(tmp.0.clone()), with_trash());
    let ids: Vec<String> = fights.iter().filter_map(|f| disk.store(f, facts)).collect();
    assert_eq!(ids.len(), 2);
    drop(disk);

    let reopened = Store::open(DirBackend::new(tmp.0.clone()), with_trash());
    assert_eq!(reopened.corrupt(), 0);
    let (_tmp2, mem, _, _, kill, trash) = spans_store();
    for id in [&kill, &trash] {
        let a = reopened.card(id).expect("read back");
        let b = mem.card(id).unwrap();
        assert_eq!(a.players, b.players, "{id}: every scalar round-trips");
        let ra = reopened.rows(id).unwrap();
        let rb = mem.rows(id).unwrap();
        assert_eq!(ra.uptime, rb.uptime, "{id}: the cells round-trip");
        assert_eq!(ra.coarse, rb.coarse, "{id}: the series round-trip");
        for guid in ROSTER {
            for view in [View::Taken, View::Healing, View::Damage] {
                let a: StoredFight = reopened.stored_fight(id, view, Some(guid)).unwrap();
                let b = mem.stored_fight(id, view, Some(guid)).unwrap();
                assert_eq!(a, b, "{id} {guid} {view:?}");
            }
        }
    }
    // The stored `am_uptime_pct` is on the file (SQL reads it) and is the
    // derived value, never read back into the card.
    let file = std::fs::read_to_string(tmp.0.join("fights").join(format!("{kill}.json"))).unwrap();
    assert!(file.contains("\"am_uptime_pct\":45"), "{file}");
    assert!(file.contains("\"am_uptime_ms\":27000"));
}

#[test]
fn the_mock_daemons_store_writes_the_same_card() {
    // The in-process fake daemon feeds every Closed into its own store —
    // the seam the GUI and TUI tests build on — so what it holds must be
    // what the engine path above holds.
    let mock = MockDaemon::fixture_at(Path::new(SPANS)).with_history();
    let store = mock.history();
    let card = store
        .cards()
        .iter()
        .find(|c| c.name == BOSS && c.kind == FightKind::Encounter)
        .expect("the mock stored the kill");
    assert_eq!(scalars(player(card, WARRIOR)), (27_000, 0, 0, 3, 58_000));
    assert_eq!(scalars(player(card, PRIEST)), (0, 3, 38_000, 1, 40_000));
    let rows = store.rows(&card.id).unwrap();
    assert_eq!(rows.uptime.len(), 3);
    assert!(
        rows.coarse
            .iter()
            .any(|c| c.guid == WARRIOR && c.taken10[0] == 22_000)
    );
}
